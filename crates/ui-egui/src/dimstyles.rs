//! The Dimension Style Manager (DIMSTYLE, Format ▸ Dimension Style...) and its style editor.
//!
//! The manager lists the drawing's dimension styles with a preview of the selected one and
//! Set Current, New..., Modify..., Override... and Compare.... The editor (New, Modify, Override)
//! shows the style's settings on seven tabs — Lines, Symbols and Arrows, Text, Fit, Primary Units,
//! Alternate Units, Tolerances — with a live preview, and changes nothing until OK:
//! Modify and New run DIMSTYLE with every setting, Override sets the changed DIM* variables
//! (SETVAR), the overrides new dimensions take on.
//!
//! The settings are edited as the style's JSON form (the [`DimStyle`] field names), so the same
//! rows serve the editor and the Properties palette ([`field_row`]).

use std::cell::RefCell;
use std::collections::HashMap;

use cadcraft_doc::DimStyle;
use cadcraft_render::DisplayList;
use egui::{Color32, RichText, Sense, Stroke, vec2};
use serde_json::{Map, Value, json};

use crate::CadApp;
use crate::theme::Tokens;

/// The arrowheads, as DIMBLK names and their labels.
pub const ARROWS: &[(&str, &str)] = &[
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
    ("_SMALL", "Dot small blank"),
    ("_BOXBLANK", "Box"),
    ("_BOXFILLED", "Box filled"),
    ("_DATUMBLANK", "Datum triangle"),
    ("_DATUMFILLED", "Datum triangle filled"),
    ("_INTEGRAL", "Integral"),
    ("_NONE", "None"),
];

const LINEAR_UNITS: &[(i64, &str)] =
    &[(1, "Scientific"), (2, "Decimal"), (3, "Engineering"), (4, "Architectural"), (5, "Fractional"), (6, "Windows Desktop")];
const ALT_UNITS: &[(i64, &str)] = &[
    (1, "Scientific"),
    (2, "Decimal"),
    (3, "Engineering"),
    (4, "Architectural Stacked"),
    (5, "Fractional Stacked"),
    (6, "Architectural"),
    (7, "Fractional"),
    (8, "Windows Desktop"),
];
const ANGULAR_UNITS: &[(i64, &str)] = &[(0, "Decimal Degrees"), (1, "Degrees Minutes Seconds"), (2, "Gradians"), (3, "Radians")];
const FRACTIONS: &[(i64, &str)] = &[(0, "Horizontal"), (1, "Diagonal"), (2, "Not Stacked")];
const TEXT_VERTICAL: &[(i64, &str)] = &[(0, "Centered"), (1, "Above"), (2, "Outside"), (3, "JIS"), (4, "Below")];
const TEXT_HORIZONTAL: &[(i64, &str)] =
    &[(0, "Centered"), (1, "At Ext Line 1"), (2, "At Ext Line 2"), (3, "Over Ext Line 1"), (4, "Over Ext Line 2")];
const FIT: &[(i64, &str)] = &[(3, "Either text or arrows (best fit)"), (1, "Arrows"), (2, "Text"), (0, "Both text and arrows")];
const TEXT_MOVE: &[(i64, &str)] =
    &[(0, "Beside the dimension line"), (1, "Over dimension line, with leader"), (2, "Over dimension line, without leader")];
const TOL_JUST: &[(i64, &str)] = &[(0, "Bottom"), (1, "Middle"), (2, "Top")];
const ARC_SYMBOL: &[(i64, &str)] = &[(0, "Preceding dimension text"), (1, "Above dimension text"), (2, "None")];
const SEPARATORS: &[(&str, &str)] = &[(".", "'.' (Period)"), (",", "',' (Comma)"), (" ", "' ' (Space)")];

/// How one setting is edited.
#[derive(Clone, Copy)]
pub enum Kind {
    /// A length or factor.
    Num(&'static str),
    /// A choice among numbered options.
    Choice(&'static str, &'static [(i64, &'static str)]),
    /// Decimal places, 0–8.
    Precision(&'static str),
    Bool(&'static str),
    /// One bit of a bit code (zero suppression).
    Bit(&'static str, u64),
    Color(&'static str),
    /// An arrowhead; the first one also sets DIMBLK.
    Arrow(&'static str),
    Lineweight(&'static str),
    Linetype(&'static str),
    TextStyle(&'static str),
    /// The text before or after the placeholder (`<>`, or `[]` for alternate units) of a
    /// prefix/suffix template.
    Prefix(&'static str, &'static str),
    Suffix(&'static str, &'static str),
    Separator(&'static str),
    /// Tolerance display: None, Symmetrical, Deviation, Limits, Basic.
    TolMethod,
    /// Text alignment: Horizontal, Aligned with dimension line, ISO standard.
    TextAlign,
    /// The frame around the text (DIMGAP negative).
    Frame,
    /// The offset from the dimension line (DIMGAP's size, keeping the frame).
    Gap,
    /// Center marks: None, Mark, Line, and the size.
    CenterType,
    CenterSize,
}

/// A labelled setting.
pub type Row = (&'static str, Kind);

/// The editor's tabs and their groups of settings.
pub const TABS: &[(&str, &[(&str, &[Row])])] = &[
    (
        "Lines",
        &[
            (
                "Dimension lines",
                &[
                    ("Color", Kind::Color("dimLineColor")),
                    ("Linetype", Kind::Linetype("dimLinetype")),
                    ("Lineweight", Kind::Lineweight("dimLineweight")),
                    ("Extend beyond ticks", Kind::Num("dimLineExtend")),
                    ("Baseline spacing", Kind::Num("baselineSpacing")),
                    ("Suppress dim line 1", Kind::Bool("suppressDim1")),
                    ("Suppress dim line 2", Kind::Bool("suppressDim2")),
                ],
            ),
            (
                "Extension lines",
                &[
                    ("Color", Kind::Color("extLineColor")),
                    ("Linetype ext line 1", Kind::Linetype("extLinetype1")),
                    ("Linetype ext line 2", Kind::Linetype("extLinetype2")),
                    ("Lineweight", Kind::Lineweight("extLineweight")),
                    ("Suppress ext line 1", Kind::Bool("suppressExt1")),
                    ("Suppress ext line 2", Kind::Bool("suppressExt2")),
                    ("Extend beyond dim lines", Kind::Num("extExtend")),
                    ("Offset from origin", Kind::Num("extOffset")),
                    ("Fixed length extension lines", Kind::Bool("fixedExt")),
                    ("Length", Kind::Num("fixedExtLength")),
                ],
            ),
        ],
    ),
    (
        "Symbols and Arrows",
        &[
            (
                "Arrowheads",
                &[
                    ("First", Kind::Arrow("arrowBlock1")),
                    ("Second", Kind::Arrow("arrowBlock2")),
                    ("Leader", Kind::Arrow("leaderArrowBlock")),
                    ("Arrow size", Kind::Num("arrowSize")),
                    ("Tick size", Kind::Num("tickSize")),
                ],
            ),
            ("Center marks", &[("Type", Kind::CenterType), ("Size", Kind::CenterSize)]),
            ("Arc length symbol", &[("Arc length symbol", Kind::Choice("arcSymbol", ARC_SYMBOL))]),
            ("Radius jog dimension", &[("Jog angle", Kind::Num("jogAngle"))]),
        ],
    ),
    (
        "Text",
        &[
            (
                "Text appearance",
                &[
                    ("Text style", Kind::TextStyle("textStyle")),
                    ("Text color", Kind::Color("textColor")),
                    ("Text height", Kind::Num("textHeight")),
                    ("Fraction height scale", Kind::Num("tolScale")),
                    ("Draw frame around text", Kind::Frame),
                ],
            ),
            (
                "Text placement",
                &[
                    ("Vertical", Kind::Choice("textAbove", TEXT_VERTICAL)),
                    ("Horizontal", Kind::Choice("textJust", TEXT_HORIZONTAL)),
                    ("Offset from dim line", Kind::Gap),
                ],
            ),
            ("Text alignment", &[("Alignment", Kind::TextAlign)]),
        ],
    ),
    (
        "Fit",
        &[
            (
                "Fit options",
                &[
                    ("Move outside the extension lines", Kind::Choice("fit", FIT)),
                    ("Always keep text between ext lines", Kind::Bool("textInside")),
                    ("Suppress arrows if they don't fit inside", Kind::Bool("suppressOutsideArrows")),
                ],
            ),
            ("Text placement", &[("When text is not in the default position", Kind::Choice("textMove", TEXT_MOVE))]),
            ("Scale for dimension features", &[("Annotative", Kind::Bool("annotative")), ("Use overall scale of", Kind::Num("scale"))]),
            ("Fine tuning", &[("Draw dim line between ext lines", Kind::Bool("forceLineInside"))]),
        ],
    ),
    (
        "Primary Units",
        &[
            (
                "Linear dimensions",
                &[
                    ("Unit format", Kind::Choice("linearUnit", LINEAR_UNITS)),
                    ("Precision", Kind::Precision("decimals")),
                    ("Fraction format", Kind::Choice("fractionFormat", FRACTIONS)),
                    ("Decimal separator", Kind::Separator("decimalSeparator")),
                    ("Round off", Kind::Num("round")),
                    ("Prefix", Kind::Prefix("post", "<>")),
                    ("Suffix", Kind::Suffix("post", "<>")),
                    ("Measurement scale factor", Kind::Num("linearFactor")),
                    ("Suppress leading zeros", Kind::Bit("zeroSuppression", 4)),
                    ("Suppress trailing zeros", Kind::Bit("zeroSuppression", 8)),
                ],
            ),
            (
                "Angular dimensions",
                &[
                    ("Units format", Kind::Choice("angularUnit", ANGULAR_UNITS)),
                    ("Precision", Kind::Precision("angularDecimals")),
                    ("Suppress leading zeros", Kind::Bit("angularZeroSuppression", 1)),
                    ("Suppress trailing zeros", Kind::Bit("angularZeroSuppression", 2)),
                ],
            ),
        ],
    ),
    (
        "Alternate Units",
        &[(
            "Alternate units",
            &[
                ("Display alternate units", Kind::Bool("alt")),
                ("Unit format", Kind::Choice("altUnit", ALT_UNITS)),
                ("Precision", Kind::Precision("altDecimals")),
                ("Multiplier for alt units", Kind::Num("altFactor")),
                ("Round distances to", Kind::Num("altRound")),
                ("Prefix", Kind::Prefix("altPost", "[]")),
                ("Suffix", Kind::Suffix("altPost", "[]")),
                ("Suppress leading zeros", Kind::Bit("altZeroSuppression", 4)),
                ("Suppress trailing zeros", Kind::Bit("altZeroSuppression", 8)),
            ],
        )],
    ),
    (
        "Tolerances",
        &[(
            "Tolerance format",
            &[
                ("Method", Kind::TolMethod),
                ("Precision", Kind::Precision("tolDecimals")),
                ("Upper value", Kind::Num("tolPlus")),
                ("Lower value", Kind::Num("tolMinus")),
                ("Scaling for height", Kind::Num("tolScale")),
                ("Vertical position", Kind::Choice("tolJust", TOL_JUST)),
                ("Suppress leading zeros", Kind::Bit("tolZeroSuppression", 4)),
                ("Suppress trailing zeros", Kind::Bit("tolZeroSuppression", 8)),
            ],
        )],
    ),
];

/// A dimension's settings in the Properties palette, by group (per-dimension overrides).
pub const PROPERTIES: &[(&str, &[Row])] = &[
    (
        "Lines & Arrows",
        &[
            ("Arrow 1", Kind::Arrow("arrowBlock1")),
            ("Arrow 2", Kind::Arrow("arrowBlock2")),
            ("Arrow size", Kind::Num("arrowSize")),
            ("Dim line color", Kind::Color("dimLineColor")),
            ("Dim line lineweight", Kind::Lineweight("dimLineweight")),
            ("Dim line 1", Kind::Bool("suppressDim1")),
            ("Dim line 2", Kind::Bool("suppressDim2")),
            ("Ext line color", Kind::Color("extLineColor")),
            ("Ext line lineweight", Kind::Lineweight("extLineweight")),
            ("Ext line 1", Kind::Bool("suppressExt1")),
            ("Ext line 2", Kind::Bool("suppressExt2")),
            ("Ext line ext", Kind::Num("extExtend")),
            ("Ext line offset", Kind::Num("extOffset")),
            ("Ext line fixed", Kind::Bool("fixedExt")),
            ("Ext line fixed length", Kind::Num("fixedExtLength")),
        ],
    ),
    (
        "Text",
        &[
            ("Text style", Kind::TextStyle("textStyle")),
            ("Text color", Kind::Color("textColor")),
            ("Text height", Kind::Num("textHeight")),
            ("Text offset", Kind::Gap),
            ("Text frame", Kind::Frame),
            ("Text pos vert", Kind::Choice("textAbove", TEXT_VERTICAL)),
            ("Text pos hor", Kind::Choice("textJust", TEXT_HORIZONTAL)),
            ("Text alignment", Kind::TextAlign),
        ],
    ),
    (
        "Fit",
        &[
            ("Dim line inside", Kind::Bool("forceLineInside")),
            ("Dim scale overall", Kind::Num("scale")),
            ("Fit", Kind::Choice("fit", FIT)),
            ("Text inside", Kind::Bool("textInside")),
            ("Text movement", Kind::Choice("textMove", TEXT_MOVE)),
        ],
    ),
    (
        "Primary Units",
        &[
            ("Dim units", Kind::Choice("linearUnit", LINEAR_UNITS)),
            ("Precision", Kind::Precision("decimals")),
            ("Decimal separator", Kind::Separator("decimalSeparator")),
            ("Dim prefix", Kind::Prefix("post", "<>")),
            ("Dim suffix", Kind::Suffix("post", "<>")),
            ("Dim roundoff", Kind::Num("round")),
            ("Dim scale linear", Kind::Num("linearFactor")),
            ("Suppress leading zeros", Kind::Bit("zeroSuppression", 4)),
            ("Suppress trailing zeros", Kind::Bit("zeroSuppression", 8)),
            ("Angle format", Kind::Choice("angularUnit", ANGULAR_UNITS)),
            ("Angle precision", Kind::Precision("angularDecimals")),
        ],
    ),
    (
        "Alternate Units",
        &[
            ("Alt enabled", Kind::Bool("alt")),
            ("Alt format", Kind::Choice("altUnit", ALT_UNITS)),
            ("Alt precision", Kind::Precision("altDecimals")),
            ("Alt multiplier", Kind::Num("altFactor")),
            ("Alt round", Kind::Num("altRound")),
            ("Alt prefix", Kind::Prefix("altPost", "[]")),
            ("Alt suffix", Kind::Suffix("altPost", "[]")),
        ],
    ),
    (
        "Tolerances",
        &[
            ("Tolerance display", Kind::TolMethod),
            ("Tolerance limit upper", Kind::Num("tolPlus")),
            ("Tolerance limit lower", Kind::Num("tolMinus")),
            ("Tolerance precision", Kind::Precision("tolDecimals")),
            ("Tolerance text height", Kind::Num("tolScale")),
            ("Tolerance pos vert", Kind::Choice("tolJust", TOL_JUST)),
        ],
    ),
];

/// The fields of `after` that differ from `before`.
pub fn changed_fields(before: &Map<String, Value>, after: &Map<String, Value>) -> Map<String, Value> {
    after.iter().filter(|(k, v)| before.get(*k) != Some(*v)).map(|(k, v)| (k.clone(), v.clone())).collect()
}

/// The drawing's names a row may offer: linetypes and text styles.
#[derive(Clone, Default)]
pub struct Names {
    pub linetypes: Vec<String>,
    pub text_styles: Vec<String>,
    /// User arrowhead blocks the drawing's dimension styles use.
    pub arrow_blocks: Vec<String>,
    /// Their shapes for the list's icons: the tip at the origin, pointing along -x, unit size.
    pub arrow_shapes: Vec<(String, cadcraft_render::ArrowGeom)>,
}

impl Names {
    pub fn of(d: &cadcraft_doc::Drawing) -> Names {
        let mut arrow_blocks: Vec<String> = Vec::new();
        let dims = d.dim_styles.iter().flat_map(|st| [&st.arrow_block, &st.arrow_block1, &st.arrow_block2, &st.leader_arrow_block]);
        let mleaders = d.mleader_styles.iter().map(|st| &st.arrow_block);
        for n in dims.chain(mleaders) {
            if !builtin_arrow(n) && cadcraft_render::arrow_block(d, n).is_some() && !arrow_blocks.iter().any(|b| b.eq_ignore_ascii_case(n)) {
                arrow_blocks.push(n.trim().to_string());
            }
        }
        let arrow_shapes = arrow_blocks
            .iter()
            .map(|n| (n.clone(), cadcraft_render::named_arrow(d, n, cadcraft_geom::Vec2::ZERO, cadcraft_geom::Vec2::new(-1.0, 0.0), 1.0)))
            .collect();
        Names {
            linetypes: d.linetypes.iter().map(|l| l.name.clone()).collect(),
            text_styles: d.text_styles.iter().map(|s| s.name.clone()).collect(),
            arrow_blocks,
            arrow_shapes,
        }
    }
}

fn num(m: &Map<String, Value>, f: &str) -> f64 {
    m.get(f).and_then(Value::as_f64).unwrap_or(0.0)
}
fn int(m: &Map<String, Value>, f: &str) -> i64 {
    m.get(f).and_then(|v| v.as_i64().or_else(|| v.as_bool().map(i64::from))).unwrap_or(0)
}
fn flag(m: &Map<String, Value>, f: &str) -> bool {
    m.get(f).and_then(Value::as_bool).unwrap_or(false)
}
fn text(m: &Map<String, Value>, f: &str) -> String {
    m.get(f).and_then(Value::as_str).unwrap_or("").to_string()
}

fn color_label(v: &Value) -> String {
    let c: cadcraft_color::Color = serde_json::from_value(v.clone()).unwrap_or_default();
    crate::i18n::t(&c.name()).to_string()
}

fn combo<T: PartialEq + Clone>(ui: &mut egui::Ui, id: impl std::hash::Hash + std::fmt::Debug, current: &T, items: &[(T, String)]) -> Option<T> {
    let shown = items.iter().find(|(v, _)| v == current).map_or_else(String::new, |(_, l)| l.clone());
    let mut picked = None;
    egui::ComboBox::from_id_salt(id)
        .width((ui.available_width() - 14.0).clamp(80.0, 190.0))
        .selected_text(crate::i18n::widget(ui, &shown, egui::TextStyle::Button))
        .height(320.0)
        .show_ui(ui, |ui| {
            for (v, l) in items {
                if ui.selectable_label(v == current, crate::i18n::widget(ui, l, egui::TextStyle::Button)).clicked() && v != current {
                    picked = Some(v.clone());
                }
            }
        });
    picked
}

/// The arrowhead list: the built-in ones, then the drawing's user arrowheads (and `cur` when it
/// is a block none of the styles use).
fn arrow_items(names: &Names, cur: &str) -> Vec<(String, String)> {
    let mut items: Vec<(String, String)> = ARROWS.iter().map(|(n, l)| (n.to_string(), crate::i18n::t(l).to_string())).collect();
    let cur = cur.trim().to_string();
    for b in names.arrow_blocks.iter().chain((!builtin_arrow(&cur)).then_some(&cur)) {
        if !items.iter().any(|(n, _)| n.eq_ignore_ascii_case(b)) {
            items.push((b.clone(), format!("{} {}", crate::tl!("User Arrow:"), b)));
        }
    }
    items
}

/// The entry of [`arrow_items`] that `cur` (any spelling of an arrowhead) is.
fn shown_arrow(names: &Names, cur: &str) -> String {
    if builtin_arrow(cur) {
        ARROWS.iter().find(|(n, _)| same_arrow(n, cur)).map_or(String::new(), |(n, _)| n.to_string())
    } else {
        arrow_items(names, cur).into_iter().find(|(n, _)| n.eq_ignore_ascii_case(cur.trim())).map_or(cur.to_string(), |(n, _)| n)
    }
}

/// An arrowhead drop-down (built-in arrowheads and the drawing's user arrowhead blocks) showing
/// `cur`; the name picked.
pub fn arrow_combo(ui: &mut egui::Ui, id: egui::Id, names: &Names, cur: &str) -> Option<String> {
    let current = shown_arrow(names, cur);
    let items = arrow_items(names, cur);
    let shown = items.iter().find(|(v, _)| *v == current).map_or_else(String::new, |(_, l)| l.clone());
    let mut picked = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (r, _) = ui.allocate_exact_size(vec2(34.0, 18.0), egui::Sense::hover());
        paint_arrow(ui.painter(), r, names, &current);
        egui::ComboBox::from_id_salt(id)
            .width((ui.available_width() - 14.0).clamp(80.0, 170.0))
            .selected_text(crate::i18n::widget(ui, &shown, egui::TextStyle::Button))
            .height(360.0)
            .show_ui(ui, |ui| {
                for (v, l) in &items {
                    ui.horizontal(|ui| {
                        let (r, _) = ui.allocate_exact_size(vec2(34.0, 18.0), egui::Sense::hover());
                        paint_arrow(ui.painter(), r, names, v);
                        if ui.selectable_label(*v == current, crate::i18n::widget(ui, l, egui::TextStyle::Button)).clicked() && *v != current {
                            picked = Some(v.clone());
                        }
                    });
                }
            });
    });
    picked
}

/// The arrowhead `name` as a small picture: a short line ending in the arrowhead at the left.
pub fn paint_arrow(p: &egui::Painter, r: egui::Rect, names: &Names, name: &str) {
    use cadcraft_geom::Vec2;
    thread_local! {
        static BLANK: cadcraft_doc::Drawing = cadcraft_doc::Drawing::default();
    }
    let g = match names.arrow_shapes.iter().find(|(n, _)| n.eq_ignore_ascii_case(name.trim())) {
        Some((_, g)) => g.clone(),
        None => BLANK.with(|d| cadcraft_render::named_arrow(d, name, Vec2::ZERO, Vec2::new(-1.0, 0.0), 1.0)),
    };
    let col = crate::theme::Tokens::get().text;
    // The arrowhead is one unit long: fit about 1.9 units (arrow and line) across the box.
    let k = f64::from(r.width()) / 2.0;
    let left = f64::from(r.left()) + 0.1 * k;
    let at = |v: &Vec2| egui::pos2((left + v.x * k) as f32, (f64::from(r.center().y) - v.y * k) as f32);
    let clip = p.with_clip_rect(r.expand(1.0));
    let stroke = egui::Stroke::new(1.2, col);
    clip.line_segment([at(&Vec2::ZERO), at(&Vec2::new(1.85, 0.0))], stroke);
    for l in &g.lines {
        if l.len() >= 2 {
            clip.add(egui::Shape::line(l.iter().map(at).collect(), stroke));
        }
    }
    let mut mesh = egui::Mesh::default();
    for t in g.tris.chunks_exact(3) {
        let base = u32::try_from(mesh.vertices.len()).unwrap_or(u32::MAX);
        for v in t {
            mesh.colored_vertex(at(v), col);
        }
        mesh.add_triangle(base, base + 1, base + 2);
    }
    if !mesh.is_empty() {
        clip.add(egui::Shape::mesh(mesh));
    }
}

/// The decimal places as AutoCAD shows them: `0`, `0.0`, `0.00`…
fn precision_items() -> Vec<(i64, String)> {
    (0..=8).map(|n| (n, if n == 0 { "0".to_string() } else { format!("0.{}", "0".repeat(n as usize)) })).collect()
}

/// The arrowhead a DIMBLK value names, for comparing names written differently.
fn same_arrow(a: &str, b: &str) -> bool {
    cadcraft_render::Arrowhead::parse(a) == cadcraft_render::Arrowhead::parse(b)
}

/// Whether a DIMBLK name is one of the arrowheads drawn in code (any of its spellings); other
/// names are user arrowheads (blocks).
pub fn builtin_arrow(name: &str) -> bool {
    let norm = |s: &str| s.trim().trim_start_matches('_').chars().filter(|c| c.is_ascii_alphanumeric()).collect::<String>().to_ascii_lowercase();
    let n = norm(name);
    n.is_empty()
        || ARROWS.iter().any(|(k, l)| norm(k) == n || norm(l) == n)
        || ["filled", "blank", "architecturaltick", "tick", "rightangle", "originindicator", "originindicator2", "box", "datumtriangle"]
            .contains(&n.as_str())
}

/// The tolerance display of a style (0 None, 1 Symmetrical, 2 Deviation, 3 Limits, 4 Basic).
fn tol_method(m: &Map<String, Value>) -> i64 {
    if flag(m, "limits") {
        3
    } else if flag(m, "tolerance") {
        if (num(m, "tolPlus") - num(m, "tolMinus")).abs() < 1e-12 { 1 } else { 2 }
    } else if num(m, "textGap") < 0.0 {
        4
    } else {
        0
    }
}

/// Split a prefix/suffix template at its placeholder.
fn split_post(post: &str, placeholder: &str) -> (String, String) {
    match post.find(placeholder) {
        Some(i) => (post[..i].to_string(), post[i + placeholder.len()..].to_string()),
        None => (String::new(), post.to_string()),
    }
}

/// One setting's control; true when it changed `m`.
pub fn field_row(ui: &mut egui::Ui, id: egui::Id, names: &Names, m: &mut Map<String, Value>, kind: Kind) -> bool {
    let before = m.clone();
    match kind {
        Kind::Num(f) => {
            let mut v = num(m, f);
            let speed = (v.abs() * 0.01).max(0.001);
            if ui.add(egui::DragValue::new(&mut v).speed(speed).max_decimals(4).min_decimals(2)).changed() && v.is_finite() {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::Choice(f, items) => {
            let items: Vec<(i64, String)> = items.iter().map(|(v, l)| (*v, crate::i18n::t(l).to_string())).collect();
            if let Some(v) = combo(ui, id.with(f), &int(m, f), &items) {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::Precision(f) => {
            if let Some(v) = combo(ui, id.with(f), &int(m, f), &precision_items()) {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::Bool(f) => {
            let mut b = flag(m, f);
            if ui.checkbox(&mut b, "").changed() {
                m.insert(f.into(), json!(b));
            }
        }
        Kind::Bit(f, bit) => {
            let v = int(m, f) as u64;
            let mut b = v & bit != 0;
            if ui.checkbox(&mut b, "").changed() {
                m.insert(f.into(), json!(if b { v | bit } else { v & !bit }));
            }
        }
        Kind::Color(f) => {
            let cur = m.get(f).cloned().unwrap_or(Value::Null);
            let mut items: Vec<(Value, String)> = vec![
                (json!({ "kind": "byBlock" }), crate::tl!("ByBlock").to_string()),
                (json!({ "kind": "byLayer" }), crate::tl!("ByLayer").to_string()),
            ];
            for (aci, name) in [(1, "Red"), (2, "Yellow"), (3, "Green"), (4, "Cyan"), (5, "Blue"), (6, "Magenta"), (7, "White")] {
                items.push((serde_json::to_value(cadcraft_color::Color::Index(aci)).unwrap_or(Value::Null), crate::i18n::t(name).to_string()));
            }
            if !items.iter().any(|(v, _)| *v == cur) {
                items.push((cur.clone(), color_label(&cur)));
            }
            if let Some(v) = combo(ui, id.with(f), &cur, &items) {
                m.insert(f.into(), v);
            }
        }
        Kind::Arrow(f) => {
            let mut cur = text(m, f);
            // An empty per-end arrowhead uses DIMBLK.
            if cur.is_empty() && f != "leaderArrowBlock" {
                cur = text(m, "arrowBlock");
            }
            let shown = shown_arrow(names, &cur);
            let shown = shown.as_str();
            if let Some(v) = arrow_combo(ui, id.with(f), names, &cur) {
                // The first arrowhead sets the second too while they match, as AutoCAD's does.
                if f == "arrowBlock1" {
                    let second = text(m, "arrowBlock2");
                    let second = if second.is_empty() { text(m, "arrowBlock") } else { second };
                    if second.eq_ignore_ascii_case(shown) || (builtin_arrow(&second) && builtin_arrow(shown) && same_arrow(&second, shown)) {
                        m.insert("arrowBlock2".into(), json!(v));
                    }
                    m.insert("arrowBlock".into(), json!(v));
                }
                m.insert(f.into(), json!(v));
            }
        }
        Kind::Lineweight(f) => {
            let mut items: Vec<(i64, String)> =
                vec![(-2, crate::tl!("ByBlock").into()), (-1, crate::tl!("ByLayer").into()), (-3, crate::tl!("Default").into())];
            items.extend(cadcraft_doc::Lineweight::STANDARD.iter().map(|v| (i64::from(*v), format!("{:.2} mm", f64::from(*v) / 100.0))));
            if let Some(v) = combo(ui, id.with(f), &int(m, f), &items) {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::Linetype(f) => {
            let mut items: Vec<(String, String)> =
                vec![(String::new(), crate::tl!("ByBlock").into()), ("ByLayer".into(), crate::tl!("ByLayer").into())];
            for n in &names.linetypes {
                if !["ByLayer", "ByBlock"].iter().any(|x| x.eq_ignore_ascii_case(n)) {
                    items.push((n.clone(), n.clone()));
                }
            }
            let cur = text(m, f);
            let cur = items.iter().find(|(v, _)| v.eq_ignore_ascii_case(&cur)).map_or(cur, |(v, _)| v.clone());
            if let Some(v) = combo(ui, id.with(f), &cur, &items) {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::TextStyle(f) => {
            let items: Vec<(String, String)> = names.text_styles.iter().map(|n| (n.clone(), n.clone())).collect();
            let cur = text(m, f);
            let cur = items.iter().find(|(v, _)| v.eq_ignore_ascii_case(&cur)).map_or(cur, |(v, _)| v.clone());
            if let Some(v) = combo(ui, id.with(f), &cur, &items) {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::Prefix(f, ph) | Kind::Suffix(f, ph) => {
            let (mut pre, mut suf) = split_post(&text(m, f), ph);
            let prefix = matches!(kind, Kind::Prefix(..));
            let r =
                ui.add(egui::TextEdit::singleline(if prefix { &mut pre } else { &mut suf }).desired_width(ui.available_width().clamp(80.0, 186.0)));
            if r.changed() {
                let post = if pre.is_empty() && suf.is_empty() { String::new() } else { format!("{pre}{ph}{suf}") };
                m.insert(f.into(), json!(post));
            }
        }
        Kind::Separator(f) => {
            let items: Vec<(String, String)> = SEPARATORS.iter().map(|(v, l)| (v.to_string(), l.to_string())).collect();
            if let Some(v) = combo(ui, id.with(f), &text(m, f), &items) {
                m.insert(f.into(), json!(v));
            }
        }
        Kind::TolMethod => {
            let items: Vec<(i64, String)> = ["None", "Symmetrical", "Deviation", "Limits", "Basic"]
                .iter()
                .enumerate()
                .map(|(i, l)| (i as i64, crate::i18n::t(l).to_string()))
                .collect();
            if let Some(v) = combo(ui, id.with("tolmethod"), &tol_method(m), &items) {
                let gap = num(m, "textGap").abs();
                m.insert("tolerance".into(), json!(v == 1 || v == 2));
                m.insert("limits".into(), json!(v == 3));
                m.insert("textGap".into(), json!(if v == 4 { -gap } else { gap }));
                if v == 1 {
                    m.insert("tolMinus".into(), json!(num(m, "tolPlus")));
                }
            }
        }
        Kind::TextAlign => {
            let (ih, oh) = (flag(m, "textInsideHorizontal"), flag(m, "textOutsideHorizontal"));
            let cur = match (ih, oh) {
                (true, _) => 0,
                (false, false) => 1,
                (false, true) => 2,
            };
            let items: Vec<(i64, String)> = ["Horizontal", "Aligned with dimension line", "ISO standard"]
                .iter()
                .enumerate()
                .map(|(i, l)| (i as i64, crate::i18n::t(l).to_string()))
                .collect();
            if let Some(v) = combo(ui, id.with("textalign"), &cur, &items) {
                m.insert("textInsideHorizontal".into(), json!(v == 0));
                m.insert("textOutsideHorizontal".into(), json!(v != 1));
            }
        }
        Kind::Frame => {
            let gap = num(m, "textGap");
            let mut b = gap < 0.0;
            if ui.checkbox(&mut b, "").changed() {
                m.insert("textGap".into(), json!(if b { -gap.abs() } else { gap.abs() }));
            }
        }
        Kind::Gap => {
            let gap = num(m, "textGap");
            let mut v = gap.abs();
            if ui.add(egui::DragValue::new(&mut v).speed(0.005).range(0.0..=1e6).max_decimals(4).min_decimals(2)).changed() {
                m.insert("textGap".into(), json!(if gap < 0.0 { -v } else { v }));
            }
        }
        Kind::CenterType => {
            let c = num(m, "centerMark");
            let cur = if c == 0.0 {
                0
            } else if c > 0.0 {
                1
            } else {
                2
            };
            let items: Vec<(i64, String)> =
                ["None", "Mark", "Line"].iter().enumerate().map(|(i, l)| (i as i64, crate::i18n::t(l).to_string())).collect();
            if let Some(v) = combo(ui, id.with("centertype"), &cur, &items) {
                let size = if c == 0.0 { 0.09 } else { c.abs() };
                m.insert(
                    "centerMark".into(),
                    json!(match v {
                        0 => 0.0,
                        1 => size,
                        _ => -size,
                    }),
                );
            }
        }
        Kind::CenterSize => {
            let c = num(m, "centerMark");
            let mut v = c.abs();
            let r = ui.add_enabled(c != 0.0, egui::DragValue::new(&mut v).speed(0.005).range(0.0..=1e6).max_decimals(4).min_decimals(2));
            if r.changed() && v > 0.0 {
                m.insert("centerMark".into(), json!(if c < 0.0 { -v } else { v }));
            }
        }
    }
    *m != before
}

/// A style's JSON form.
pub fn style_map(st: &DimStyle) -> Map<String, Value> {
    match serde_json::to_value(st) {
        Ok(Value::Object(m)) => m,
        _ => Map::new(),
    }
}

/// The style a JSON form describes (unknown or bad values keep the defaults).
pub fn map_style(m: &Map<String, Value>) -> DimStyle {
    let mut st = DimStyle::default();
    let mut fields = m.clone();
    fields.remove("name");
    st.apply_fields(&fields);
    st.name = text(m, "name");
    st
}

// ---------------------------------------------------------------- preview

thread_local! {
    static PREVIEWS: RefCell<HashMap<String, Option<DisplayList>>> = RefCell::new(HashMap::new());
}

/// The preview drawing of `st`: a part with linear, aligned, radius and angular dimensions in
/// that style, sized to its text height.
fn build_preview(st: &DimStyle, src: Option<&cadcraft_doc::Drawing>) -> Option<DisplayList> {
    let mut s = cadcraft_engine::Session::new();
    // The drawing's own blocks (user arrowheads), text styles (fonts), linetypes and layers, so
    // the preview draws the style as the drawing's dimensions are drawn.
    if let Some(src) = src
        && let Ok(d) = s.doc_mut()
    {
        d.blocks = src.blocks.clone();
        d.text_styles = src.text_styles.clone();
        d.linetypes = src.linetypes.clone();
        d.layers = src.layers.iter().map(|l| cadcraft_doc::Layer { frozen: false, on: true, ..l.clone() }).collect();
        d.handseed = d.handseed.max(src.handseed);
        for k in ["LTSCALE", "PSLTSCALE", "MEASUREMENT", "INSUNITS", "DIMASSOC"] {
            if let Some(v) = src.header.get(k) {
                d.header.set(k, v.clone());
            }
        }
    }
    let mut fields = style_map(st);
    fields.insert("name".into(), json!("Preview"));
    fields.insert("current".into(), json!(true));
    fields.insert("annotative".into(), json!(false));
    s.execute("dimstyle", &Value::Object(fields)).ok()?;
    let th = st.text_height * if st.scale > 0.0 { st.scale } else { 1.0 };
    let f = if th.is_finite() && th > 0.0 { th / 0.18 } else { 1.0 };
    let p = |x: f64, y: f64| json!([x * f, y * f]);
    let _ = s.execute("layer.new", &json!({ "name": "CADCraft preview part", "color": 40, "current": true }));
    let outline = [(0.0, 0.0), (3.0, 0.0), (2.0, 1.7), (1.0, 1.7), (1.0, 2.2), (0.0, 2.2), (0.0, 0.0)];
    s.execute("line", &json!({ "points": outline.iter().map(|(x, y)| p(*x, *y)).collect::<Vec<_>>() })).ok()?;
    let circle = s.execute("circle", &json!({ "center": p(0.55, 0.85), "radius": 0.4 * f })).ok()?;
    let _ = s.execute("layer.new", &json!({ "name": "CADCraft preview dims", "color": 7, "current": true }));
    let _ = s.execute("dimlinear", &json!({ "p1": p(0.0, 2.2), "p2": p(1.0, 2.2), "at": p(0.5, 2.75) }));
    let _ = s.execute("dimlinear", &json!({ "p1": p(0.0, 0.0), "p2": p(0.0, 2.2), "at": p(-0.55, 1.1) }));
    let _ = s.execute("dimaligned", &json!({ "p1": p(3.0, 0.0), "p2": p(2.0, 1.7), "at": p(3.05, 1.2) }));
    if let Some(h) = circle["handle"].as_str() {
        let _ = s.execute("dimradius", &json!({ "handle": h, "at": p(0.05, 0.35) }));
    }
    let _ = s.execute("dimangular", &json!({ "vertex": p(3.0, 0.0), "p1": p(0.0, 0.0), "p2": p(2.0, 1.7), "at": p(2.2, 0.45) }));
    let d = s.doc().ok()?;
    Some(cadcraft_render::build(d, &cadcraft_doc::Space::Model, &cadcraft_render::Options::default()))
}

/// Draw the preview of `st` in a `size` box.
pub fn preview(ui: &mut egui::Ui, st: &DimStyle, size: egui::Vec2, src: Option<&cadcraft_doc::Drawing>) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
    let p = ui.painter_at(rect);
    p.rect_filled(rect, 2.0, t.canvas);
    let key = format!("{:p}{}", src.map_or(std::ptr::null(), |d| d as *const _), serde_json::to_string(st).unwrap_or_default());
    let list = PREVIEWS.with(|c| {
        let mut c = c.borrow_mut();
        if c.len() > 64 {
            c.clear();
        }
        c.entry(key).or_insert_with(|| build_preview(st, src)).clone()
    });
    let Some(list) = list else { return };
    let b = list.bounds;
    if b.is_empty() || !(b.width() > 0.0 && b.height() > 0.0) {
        return;
    }
    let inner = rect.shrink(12.0);
    let scale = (f64::from(inner.width()) / b.width()).min(f64::from(inner.height()) / b.height());
    let xf = crate::canvas::Xf { rect, center: b.center(), scale };
    let bg = cadcraft_color::Rgb(t.canvas.r(), t.canvas.g(), t.canvas.b());
    crate::canvas::draw_list(&p, &xf, &list, bg, false);
}

// ---------------------------------------------------------------- the manager

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Modify,
    New,
    Override,
}

#[derive(Clone)]
struct Editor {
    mode: Mode,
    name: String,
    base: Map<String, Value>,
    style: Map<String, Value>,
    tab: usize,
}

#[derive(Clone)]
struct NewStyle {
    name: String,
    from: String,
    annotative: bool,
}

#[derive(Clone, Default)]
struct State {
    sel: Option<String>,
    in_use_only: bool,
    new: Option<NewStyle>,
    editor: Option<Editor>,
    compare: Option<(String, String)>,
    rename: Option<(String, String)>,
}

fn state_id() -> egui::Id {
    egui::Id::new("cc_dimstyle_manager")
}

/// Dimension style names used by dimensions, leaders and tolerances anywhere in the drawing.
fn styles_in_use(d: &cadcraft_doc::Drawing) -> Vec<String> {
    use cadcraft_doc::EntityKind;
    let mut out: Vec<String> = Vec::new();
    let spaces = std::iter::once(cadcraft_doc::Space::Model).chain(d.layouts.iter().map(|l| cadcraft_doc::Space::Paper(l.name.clone())));
    for space in spaces {
        let Some(st) = d.space(&space) else { continue };
        for e in st.iter() {
            let name = match &e.kind {
                EntityKind::Dimension(dm) => &dm.style,
                EntityKind::Leader(l) => &l.style,
                EntityKind::Tolerance(t) => &t.style,
                _ => continue,
            };
            if !out.iter().any(|n| n.eq_ignore_ascii_case(name)) {
                out.push(name.clone());
            }
        }
    }
    out
}

/// A small blue mark for annotative styles, as the style lists show it.
fn annotative_mark(ui: &mut egui::Ui, on: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
    if on {
        let c = Color32::from_rgb(70, 140, 230);
        let m = r.center();
        for a in [-90.0f32, 30.0, 150.0] {
            let d = vec2(a.to_radians().cos(), a.to_radians().sin()) * 6.0;
            ui.painter().line_segment([m, m + d], Stroke::new(2.0, c));
        }
    }
}

/// Format ▸ Dimension Style...: the manager, and the editor or New / Compare windows it opens.
pub fn dialog(app: &mut CadApp, ctx: &egui::Context, open: &mut bool) {
    let t = Tokens::get();
    let Ok(d) = app.session.doc() else {
        *open = false;
        return;
    };
    // The drawing the previews take user arrowheads, text styles and linetypes from.
    let src = app.session.state().ok().map(|s| s.doc.clone());
    let src = src.as_deref();
    let mut st: State = ctx.data(|m| m.get_temp(state_id())).unwrap_or_default();
    let current = d.header.str("DIMSTYLE", "Standard");
    let used = styles_in_use(d);
    let styles: Vec<DimStyle> = d.dim_styles.clone();
    let names = Names::of(d);
    let overrides = d.dim_overrides();
    let sel = st.sel.clone().filter(|s| styles.iter().any(|x| x.name.eq_ignore_ascii_case(s))).unwrap_or_else(|| current.clone());
    let sel_style = styles.iter().find(|x| x.name.eq_ignore_ascii_case(&sel)).cloned().unwrap_or_default();
    let mut run: Vec<(&'static str, Value)> = Vec::new();
    let modal = st.editor.is_some() || st.new.is_some() || st.compare.is_some();

    egui::Window::new(crate::tl!("Dimension Style Manager"))
        .id(egui::Id::new("cc_dimstyle_window"))
        .open(open)
        .resizable(false)
        .collapsible(false)
        .pivot(egui::Align2::CENTER_CENTER)
        .default_pos(ctx.content_rect().center())
        .show(ctx, |ui| {
            ui.add_enabled_ui(!modal, |ui| {
                ui.label(format!("{} {current}", crate::tl!("Current dimension style:")));
                ui.add_space(4.0);
                ui.horizontal_top(|ui| {
                    // Styles and the List filter.
                    ui.vertical(|ui| {
                        ui.set_width(220.0);
                        ui.label(crate::tl!("Styles:"));
                        egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(4.0).show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("cc_dimstyle_list")
                                .max_height(230.0)
                                .min_scrolled_height(230.0)
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    for s in styles.iter().filter(|s| {
                                        !st.in_use_only
                                            || used.iter().any(|u| u.eq_ignore_ascii_case(&s.name))
                                            || s.name.eq_ignore_ascii_case(&current)
                                    }) {
                                        ui.horizontal(|ui| {
                                            annotative_mark(ui, s.annotative);
                                            let mut label = RichText::new(&s.name);
                                            if s.name.eq_ignore_ascii_case(&current) {
                                                label = label.strong();
                                            }
                                            let r = ui.selectable_label(s.name.eq_ignore_ascii_case(&sel), label);
                                            if r.clicked() {
                                                st.sel = Some(s.name.clone());
                                            }
                                            if r.double_clicked() {
                                                run.push(("dimstyle.current", json!({ "name": s.name })));
                                            }
                                            r.context_menu(|ui| {
                                                if ui.button(crate::tl!("Set Current")).clicked() {
                                                    run.push(("dimstyle.current", json!({ "name": s.name })));
                                                    ui.close();
                                                }
                                                if ui.button(crate::tl!("Rename")).clicked() {
                                                    st.rename = Some((s.name.clone(), s.name.clone()));
                                                    ui.close();
                                                }
                                                let deletable = !s.name.eq_ignore_ascii_case("Standard")
                                                    && !s.name.eq_ignore_ascii_case(&current)
                                                    && !used.iter().any(|u| u.eq_ignore_ascii_case(&s.name));
                                                if ui.add_enabled(deletable, egui::Button::new(crate::tl!("Delete"))).clicked() {
                                                    run.push(("dimstyle.delete", json!({ "name": s.name })));
                                                    ui.close();
                                                }
                                            });
                                        });
                                    }
                                });
                        });
                        if let Some((from, mut to)) = st.rename.clone() {
                            ui.horizontal(|ui| {
                                let r = ui.add(egui::TextEdit::singleline(&mut to).desired_width(150.0));
                                r.request_focus();
                                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                                    if !to.trim().is_empty() && to.trim() != from {
                                        run.push(("dimstyle.rename", json!({ "from": from, "to": to.trim() })));
                                        st.sel = Some(to.trim().to_string());
                                    }
                                    st.rename = None;
                                } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                                    st.rename = None;
                                } else {
                                    st.rename = Some((from.clone(), to.clone()));
                                }
                            });
                        }
                        ui.add_space(4.0);
                        ui.label(crate::tl!("List:"));
                        let items = vec![(false, crate::tl!("All styles").to_string()), (true, crate::tl!("Styles in use").to_string())];
                        if let Some(v) = combo(ui, "cc_dimstyle_listfilter", &st.in_use_only, &items) {
                            st.in_use_only = v;
                        }
                    });
                    // Preview and description.
                    ui.vertical(|ui| {
                        ui.label(format!("{} {sel}", crate::tl!("Preview of:")));
                        preview(ui, &sel_style, vec2(320.0, 230.0), src);
                        ui.add_space(4.0);
                        ui.label(crate::tl!("Description"));
                        egui::Frame::NONE.stroke(Stroke::new(1.0, t.border)).inner_margin(6.0).show(ui, |ui| {
                            ui.set_width(308.0);
                            ui.set_min_height(48.0);
                            let mut desc = sel.clone();
                            if sel_style.annotative {
                                desc.push_str(&format!("  ({})", crate::tl!("Annotative")));
                            }
                            ui.label(desc);
                            if sel.eq_ignore_ascii_case(&current) && !overrides.is_empty() {
                                ui.label(RichText::new(format!("+ {} {}", overrides.len(), crate::tl!("overrides"))).color(t.text_dim));
                            }
                        });
                    });
                    // Buttons.
                    ui.vertical(|ui| {
                        let w = vec2(120.0, 26.0);
                        if ui.add_enabled(!sel.eq_ignore_ascii_case(&current), egui::Button::new(crate::tl!("Set Current")).min_size(w)).clicked() {
                            run.push(("dimstyle.current", json!({ "name": sel })));
                        }
                        if ui.add(egui::Button::new(crate::tl!("New...")).min_size(w)).clicked() {
                            st.new = Some(NewStyle {
                                name: format!("{} {sel}", crate::tl!("Copy of")),
                                from: sel.clone(),
                                annotative: sel_style.annotative,
                            });
                        }
                        if ui.add(egui::Button::new(crate::tl!("Modify...")).min_size(w)).clicked() {
                            let m = style_map(&sel_style);
                            st.editor = Some(Editor { mode: Mode::Modify, name: sel.clone(), base: m.clone(), style: m, tab: 0 });
                        }
                        let cur_style = styles.iter().find(|x| x.name.eq_ignore_ascii_case(&current)).cloned().unwrap_or_default();
                        if ui
                            .add_enabled(sel.eq_ignore_ascii_case(&current), egui::Button::new(crate::tl!("Override...")).min_size(w))
                            .on_disabled_hover_text(crate::tl!("Overrides apply to the current style"))
                            .clicked()
                        {
                            let base = style_map(&cur_style);
                            let mut with = base.clone();
                            with.extend(overrides.clone());
                            st.editor = Some(Editor { mode: Mode::Override, name: current.clone(), base, style: with, tab: 0 });
                        }
                        if ui.add(egui::Button::new(crate::tl!("Compare...")).min_size(w)).clicked() {
                            st.compare = Some((sel.clone(), current.clone()));
                        }
                    });
                });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(crate::tl!("Close")).min_size(vec2(90.0, 24.0))).clicked() {
                        st.compare = None;
                        st.editor = None;
                        st.new = None;
                        ctx.data_mut(|m| m.insert_temp(egui::Id::new("cc_dimstyle_close"), true));
                    }
                });
            });
        });
    if ctx.data_mut(|m| m.remove_temp::<bool>(egui::Id::new("cc_dimstyle_close"))).is_some() {
        *open = false;
    }

    // New Dimension Style: the name, the style to start with, annotative.
    if let Some(mut n) = st.new.clone() {
        let mut keep = true;
        let mut go = false;
        egui::Window::new(crate::tl!("Create New Dimension Style"))
            .id(egui::Id::new("cc_dimstyle_new"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                egui::Grid::new("cc_dimstyle_new_grid").num_columns(2).spacing(vec2(10.0, 8.0)).show(ui, |ui| {
                    ui.label(crate::tl!("New Style Name:"));
                    ui.add(egui::TextEdit::singleline(&mut n.name).desired_width(220.0));
                    ui.end_row();
                    ui.label(crate::tl!("Start With:"));
                    let items: Vec<(String, String)> = styles.iter().map(|s| (s.name.clone(), s.name.clone())).collect();
                    if let Some(v) = combo(ui, "cc_dimstyle_new_from", &n.from, &items) {
                        n.from = v;
                    }
                    ui.end_row();
                    ui.label(crate::tl!("Annotative"));
                    ui.checkbox(&mut n.annotative, "");
                    ui.end_row();
                });
                let taken = styles.iter().any(|s| s.name.eq_ignore_ascii_case(n.name.trim()));
                if taken {
                    ui.label(RichText::new(crate::tl!("A style with this name already exists.")).color(t.warn));
                }
                ui.horizontal(|ui| {
                    if ui.add_enabled(!n.name.trim().is_empty() && !taken, egui::Button::new(crate::tl!("Continue"))).clicked() {
                        go = true;
                    }
                    if ui.button(crate::tl!("Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
        if go {
            let from = styles.iter().find(|x| x.name.eq_ignore_ascii_case(&n.from)).cloned().unwrap_or_default();
            let mut m = style_map(&from);
            m.insert("name".into(), json!(n.name.trim()));
            m.insert("annotative".into(), json!(n.annotative));
            st.editor = Some(Editor { mode: Mode::New, name: n.name.trim().to_string(), base: m.clone(), style: m, tab: 0 });
            st.new = None;
        } else {
            st.new = keep.then_some(n);
        }
    }

    // The style editor.
    if let Some(mut ed) = st.editor.clone() {
        let mut close = false;
        let mut ok = false;
        let title = match ed.mode {
            Mode::Modify => format!("{} {}", crate::tl!("Modify Dimension Style:"), ed.name),
            Mode::New => format!("{} {}", crate::tl!("New Dimension Style:"), ed.name),
            Mode::Override => format!("{} {}", crate::tl!("Override Current Style:"), ed.name),
        };
        egui::Window::new(title)
            .id(egui::Id::new("cc_dimstyle_editor"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                ui.horizontal_wrapped(|ui| {
                    for (i, (name, _)) in TABS.iter().enumerate() {
                        if ui.selectable_label(ed.tab == i, crate::i18n::t(name)).clicked() {
                            ed.tab = i;
                        }
                    }
                });
                ui.separator();
                ui.horizontal_top(|ui| {
                    ui.allocate_ui_with_layout(vec2(480.0, 430.0), egui::Layout::top_down(egui::Align::Min), |ui| {
                        ui.set_min_size(vec2(480.0, 430.0));
                        egui::ScrollArea::vertical().id_salt(("cc_dimstyle_tab", ed.tab)).max_height(430.0).auto_shrink([false, false]).show(
                            ui,
                            |ui| {
                                let (_, groups) = TABS.get(ed.tab).copied().unwrap_or(TABS[0]);
                                for (group, rows) in groups {
                                    ui.label(RichText::new(crate::i18n::t(group)).strong());
                                    egui::Grid::new(("cc_dimstyle_grid", ed.tab, *group))
                                        .num_columns(2)
                                        .spacing(vec2(12.0, 6.0))
                                        .min_col_width(200.0)
                                        .show(ui, |ui| {
                                            for (label, kind) in rows.iter() {
                                                ui.label(crate::i18n::t(label));
                                                field_row(
                                                    ui,
                                                    egui::Id::new(("cc_dimstyle_field", ed.tab, *group, *label)),
                                                    &names,
                                                    &mut ed.style,
                                                    *kind,
                                                );
                                                ui.end_row();
                                            }
                                        });
                                    ui.add_space(8.0);
                                }
                            },
                        );
                    });
                    ui.vertical(|ui| {
                        preview(ui, &map_style(&ed.style), vec2(300.0, 240.0), src);
                        if ed.mode == Mode::Override {
                            ui.label(RichText::new(crate::tl!("Overrides apply to new dimensions in the current style.")).small().color(t.text_dim));
                        }
                    });
                });
                ui.separator();
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add(egui::Button::new(crate::tl!("Cancel")).min_size(vec2(80.0, 24.0))).clicked() {
                        close = true;
                    }
                    if ui.add(egui::Button::new(crate::tl!("OK")).min_size(vec2(80.0, 24.0))).clicked() {
                        ok = true;
                    }
                });
            });
        if ok {
            match ed.mode {
                Mode::Modify | Mode::New => {
                    let mut p = ed.style.clone();
                    p.insert("name".into(), json!(ed.name));
                    p.insert("current".into(), json!(ed.mode == Mode::Modify && ed.name.eq_ignore_ascii_case(&current)));
                    run.push(("dimstyle", Value::Object(p)));
                    st.sel = Some(ed.name.clone());
                }
                Mode::Override => {
                    for (var, field) in cadcraft_doc::DIMVARS {
                        let (Some(new), old) = (ed.style.get(*field), ed.base.get(*field)) else { continue };
                        let was = overrides.get(*field).or(old);
                        if Some(new) == was {
                            continue;
                        }
                        let value = match new {
                            Value::Bool(b) => json!(i64::from(*b)),
                            Value::Object(_) => {
                                let c: cadcraft_color::Color = serde_json::from_value(new.clone()).unwrap_or_default();
                                json!(c.to_aci())
                            }
                            v => v.clone(),
                        };
                        run.push(("setvar", json!({ "name": var, "value": value })));
                    }
                }
            }
            st.editor = None;
        } else if close {
            st.editor = None;
        } else {
            st.editor = Some(ed);
        }
    }

    // Compare two styles: the variables that differ.
    if let Some((mut a, mut b)) = st.compare.clone() {
        let mut keep = true;
        egui::Window::new(crate::tl!("Compare Dimension Styles"))
            .id(egui::Id::new("cc_dimstyle_compare"))
            .collapsible(false)
            .resizable(false)
            .order(egui::Order::Foreground)
            .pivot(egui::Align2::CENTER_CENTER)
            .default_pos(ctx.content_rect().center())
            .show(ctx, |ui| {
                let items: Vec<(String, String)> = styles.iter().map(|s| (s.name.clone(), s.name.clone())).collect();
                egui::Grid::new("cc_dimstyle_cmp_pick").num_columns(2).show(ui, |ui| {
                    ui.label(crate::tl!("Compare:"));
                    if let Some(v) = combo(ui, "cc_dimstyle_cmp_a", &a, &items) {
                        a = v;
                    }
                    ui.end_row();
                    ui.label(crate::tl!("With:"));
                    if let Some(v) = combo(ui, "cc_dimstyle_cmp_b", &b, &items) {
                        b = v;
                    }
                    ui.end_row();
                });
                ui.separator();
                let ma = styles.iter().find(|x| x.name.eq_ignore_ascii_case(&a)).map(style_map).unwrap_or_default();
                let mb = styles.iter().find(|x| x.name.eq_ignore_ascii_case(&b)).map(style_map).unwrap_or_default();
                let diffs: Vec<(&str, String, String)> = cadcraft_doc::DIMVARS
                    .iter()
                    .filter(|(_, f)| ma.get(*f) != mb.get(*f))
                    .map(|(v, f)| (*v, show_value(ma.get(*f)), show_value(mb.get(*f))))
                    .collect();
                if diffs.is_empty() {
                    ui.label(crate::tl!("The two styles are the same."));
                } else {
                    ui.label(format!("{} {}", crate::tl!("Differences found:"), diffs.len()));
                    egui::ScrollArea::vertical().max_height(300.0).show(ui, |ui| {
                        egui::Grid::new("cc_dimstyle_cmp").striped(true).num_columns(3).spacing(vec2(16.0, 4.0)).show(ui, |ui| {
                            ui.label(RichText::new(crate::tl!("Variable")).strong());
                            ui.label(RichText::new(&a).strong());
                            ui.label(RichText::new(&b).strong());
                            ui.end_row();
                            for (v, x, y) in &diffs {
                                ui.monospace(*v);
                                ui.label(x);
                                ui.label(y);
                                ui.end_row();
                            }
                        });
                    });
                }
                ui.separator();
                if ui.button(crate::tl!("Close")).clicked() {
                    keep = false;
                }
            });
        st.compare = keep.then_some((a, b));
    }

    ctx.data_mut(|m| m.insert_temp(state_id(), st));
    for (cmd, p) in run {
        // Errors are echoed on the command line by `run`.
        let _ = app.run(cmd, p);
    }
}

fn show_value(v: Option<&Value>) -> String {
    match v {
        None => String::new(),
        Some(Value::String(s)) if s.is_empty() => "\"\"".into(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Object(_)) => v.map(color_label).unwrap_or_default(),
        Some(v) => v.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A user arrowhead (a block of the drawing) shows in the preview: a big square block as
    /// both arrows adds geometry the built-in arrowheads don't have.
    #[test]
    fn the_preview_draws_the_drawings_user_arrowheads() {
        let mut s = cadcraft_engine::Session::new();
        let r = s.execute("rectang", &json!({ "p1": [-0.5, -0.5], "p2": [0.5, 0.5] })).unwrap();
        let h = r["handle"].as_str().unwrap().to_string();
        s.execute("block", &json!({ "name": "MYARROW", "base": [0, 0], "handles": [h], "keep": "delete" })).unwrap();
        let mut st = s.doc().unwrap().dim_style("Standard").cloned().unwrap();
        st.apply_fields(&serde_json::Map::from_iter([("arrowBlock".to_string(), json!("MYARROW"))]));
        let plain = build_preview(&st, None).unwrap();
        let with = build_preview(&st, Some(s.doc().unwrap())).unwrap();
        // The square arrows: closed polylines of five points in the preview drawn with the
        // drawing's blocks, none without them.
        let squares =
            |l: &DisplayList| l.prims.iter().filter(|p| matches!(p.kind, cadcraft_render::Kind::Polyline) && l.points(p).len() == 5).count();
        assert!(squares(&with) > squares(&plain), "{} {}", squares(&with), squares(&plain));
    }
}
