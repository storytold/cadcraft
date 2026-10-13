//! What the Properties palette shows for a selection: groups of rows, each with the
//! `properties.set` key that changes it, the value the objects share (or *VARIES*), how the
//! value reads in the drawing's units, and whether it can be edited. `properties.describe`
//! returns the same description, so the palette, Quick Properties and agents read one list.
//!
//! Text, MText and Dimension have their own groups; other objects have the General group only
//! (the palette draws their rows itself). Dimension rows are the dimension style variables,
//! valued as the dimension draws them (its style with its overrides); setting one stores a
//! per-dimension override, as AutoCAD's Properties palette does (DIMOVERRIDE).

use std::collections::HashSet;

use cadcraft_doc::{DimKind, DimStyle, Dimension, Drawing, Entity, EntityKind, HAlign, Handle, Lineweight, MText, Text, Transparency, VAlign};
use serde::Serialize;
use serde_json::{Value, json};

use crate::units;

/// What a row shows when the described objects don't share a value.
pub const VARIES: &str = "*VARIES*";

/// MTEXT line pitch per unit of text height at line spacing factor 1 (the layout's 5/3 in
/// `cadcraft_fonts`): Line space distance = factor × text height × `LINE_PITCH`.
pub const LINE_PITCH: f64 = 5.0 / 3.0;

/// Above this many dimensions, their measurement and text position aren't worked out (each
/// needs the dimension's geometry); the rows show *VARIES*.
const GEOMETRY_CAP: usize = 256;

/// The Properties of a selection.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Description {
    /// The object types selected and how many of each, in the order first selected.
    pub types: Vec<TypeCount>,
    /// The type the description is limited to; `None`: every selected object.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filter: Option<&'static str>,
    /// How many objects the rows describe.
    pub count: usize,
    pub groups: Vec<Group>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct TypeCount {
    #[serde(rename = "type")]
    pub name: &'static str,
    pub count: usize,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Group {
    pub name: &'static str,
    pub rows: Vec<Row>,
}

/// One property of the described objects.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Row {
    /// The `properties.set` key that changes it.
    pub key: &'static str,
    pub label: &'static str,
    pub kind: Kind,
    /// The value every described object has; `null` when they differ.
    pub value: Value,
    pub varies: bool,
    /// The value as shown: lengths in the drawing's units (LUNITS/LUPREC), angles in its angle
    /// units (AUNITS/AUPREC), choices by their label; *VARIES* when the objects differ.
    pub display: String,
    pub editable: bool,
    /// Why the row can't be edited, or what decides its value.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Dimension rows: a described dimension overrides its style here.
    #[serde(skip_serializing_if = "is_false")]
    pub overridden: bool,
    /// The values a choice or style row can take.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub options: Vec<Choice>,
    /// One of the rows Quick Properties shows.
    #[serde(skip_serializing_if = "is_false")]
    pub quick: bool,
}

fn is_false(b: &bool) -> bool {
    !*b
}

impl Row {
    fn quick(mut self) -> Self {
        self.quick = true;
        self
    }
    fn read_only(mut self) -> Self {
        self.editable = false;
        self
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Choice {
    pub value: Value,
    pub label: String,
}

impl Choice {
    fn new(value: Value, label: impl Into<String>) -> Self {
        Choice { value, label: label.into() }
    }
}

/// How a row's value is entered and shown.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Kind {
    /// One line of text.
    Text,
    /// MTEXT contents, inline formatting codes included.
    Contents,
    /// A plain number (factors, scales).
    Number,
    /// A length in drawing units.
    Distance,
    /// An angle in degrees.
    Angle,
    Bool,
    /// One of `options`.
    Choice,
    Color,
    Layer,
    Linetype,
    Lineweight,
    Transparency,
    TextStyle,
    DimStyle,
}

/// A transparency as `properties.set` takes it: "ByLayer", "ByBlock" or a percentage.
pub fn transparency_value(t: Transparency) -> Value {
    match t {
        Transparency::Percent(p) => json!(p),
        other => json!(other.name()),
    }
}

/// The Properties of the objects `handles` (model space and layouts), limited to the objects
/// of type `filter` (a name from [`Description::types`]) when it names one. Handles that don't
/// name an object are left out.
pub fn describe(d: &Drawing, handles: &[Handle], filter: Option<&str>) -> Description {
    let mut seen = HashSet::new();
    let all: Vec<&Entity> = handles.iter().filter(|h| seen.insert(**h)).filter_map(|h| d.entity(*h).map(|e| &**e)).collect();
    let mut types: Vec<TypeCount> = Vec::new();
    for e in &all {
        let name = e.kind.type_name();
        match types.iter_mut().find(|t| t.name == name) {
            Some(t) => t.count += 1,
            None => types.push(TypeCount { name, count: 1 }),
        }
    }
    let filter = filter.and_then(|f| types.iter().find(|t| t.name.eq_ignore_ascii_case(f.trim())).map(|t| t.name));
    let ents: Vec<&Entity> = all.into_iter().filter(|e| filter.is_none_or(|f| e.kind.type_name() == f)).collect();
    let c = Ctx::new(d);
    let mut groups = Vec::new();
    if !ents.is_empty() {
        groups.push(general(&c, &ents));
        let texts: Vec<&Text> = ents.iter().filter_map(|e| if let EntityKind::Text(t) = &e.kind { Some(t) } else { None }).collect();
        let mtexts: Vec<&MText> = ents.iter().filter_map(|e| if let EntityKind::MText(t) = &e.kind { Some(t) } else { None }).collect();
        let dims: Vec<&Dimension> = ents.iter().filter_map(|e| if let EntityKind::Dimension(t) = &e.kind { Some(t) } else { None }).collect();
        // Type groups only when every described object is of that type (AutoCAD shows the
        // common properties of a mixed selection).
        if texts.len() == ents.len() {
            groups.extend(text_groups(&c, &ents, &texts));
        } else if mtexts.len() == ents.len() {
            groups.extend(mtext_groups(&c, &ents, &mtexts));
        } else if dims.len() == ents.len() {
            groups.extend(dimension_groups(&c, &ents, &dims));
        }
    }
    Description { types, filter, count: ents.len(), groups }
}

/// Unit settings and the drawing, for formatting rows.
struct Ctx<'a> {
    d: &'a Drawing,
    lunits: i64,
    luprec: i64,
    aunits: i64,
    auprec: i64,
}

impl<'a> Ctx<'a> {
    fn new(d: &'a Drawing) -> Self {
        let h = &d.header;
        Ctx { d, lunits: h.i64("LUNITS", 2), luprec: h.i64("LUPREC", 4), aunits: h.i64("AUNITS", 0), auprec: h.i64("AUPREC", 0) }
    }

    fn distance(&self, v: f64) -> String {
        units::format_distance(v, self.lunits, self.luprec)
    }

    /// `deg` in the drawing's angle units.
    fn angle(&self, deg: f64) -> String {
        units::format_angle(deg.to_radians(), self.aunits, self.auprec)
    }

    fn show(&self, kind: Kind, v: &Value, options: &[Choice]) -> String {
        match kind {
            Kind::Distance => v.as_f64().map(|x| self.distance(x)).unwrap_or_default(),
            Kind::Number => v.as_f64().map(|x| format!("{x:.4}")).unwrap_or_default(),
            Kind::Angle => v.as_f64().map(|x| self.angle(x)).unwrap_or_default(),
            Kind::Bool => (if v.as_bool() == Some(true) { "Yes" } else { "No" }).to_string(),
            Kind::Choice => options.iter().find(|o| o.value == *v).map(|o| o.label.clone()).unwrap_or_else(|| plain(v)),
            _ => plain(v),
        }
    }

    /// A row valued by `values` (one per described object).
    fn row(&self, key: &'static str, label: &'static str, kind: Kind, values: impl IntoIterator<Item = Value>) -> Row {
        self.choice(key, label, kind, Vec::new(), values)
    }

    /// A row with options (a choice, or a style picked from the drawing's styles).
    fn choice(&self, key: &'static str, label: &'static str, kind: Kind, options: Vec<Choice>, values: impl IntoIterator<Item = Value>) -> Row {
        let mut it = values.into_iter();
        let first = it.next().unwrap_or(Value::Null);
        let varies = it.any(|v| v != first);
        let (value, display) = if varies { (Value::Null, VARIES.to_string()) } else { (first.clone(), self.show(kind, &first, &options)) };
        Row { key, label, kind, value, varies, display, editable: true, note: None, overridden: false, options, quick: false }
    }

    fn text_styles(&self) -> Vec<Choice> {
        self.d.text_styles.iter().map(|t| Choice::new(json!(t.name), t.name.clone())).collect()
    }

    fn dim_styles(&self) -> Vec<Choice> {
        self.d.dim_styles.iter().map(|t| Choice::new(json!(t.name), t.name.clone())).collect()
    }
}

/// A value as plain text: strings as they are, numbers and booleans as JSON writes them.
fn plain(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn general(c: &Ctx, ents: &[&Entity]) -> Group {
    let rows = vec![
        c.row("color", "Color", Kind::Color, ents.iter().map(|e| json!(e.common.color.name()))),
        c.row("layer", "Layer", Kind::Layer, ents.iter().map(|e| json!(e.common.layer))).quick(),
        c.row("linetype", "Linetype", Kind::Linetype, ents.iter().map(|e| json!(e.common.linetype))),
        c.row("ltscale", "Linetype scale", Kind::Number, ents.iter().map(|e| json!(e.common.ltscale))),
        c.row("lineweight", "Lineweight", Kind::Lineweight, ents.iter().map(|e| json!(e.common.lineweight.name()))),
        c.row("transparency", "Transparency", Kind::Transparency, ents.iter().map(|e| transparency_value(e.common.transparency))),
    ];
    Group { name: "General", rows }
}

fn annotative(c: &Ctx, ents: &[&Entity]) -> Row {
    c.row("annotative", "Annotative", Kind::Bool, ents.iter().map(|e| json!(cadcraft_doc::is_annotative(e)))).quick()
}

/// TEXT justifications as `properties.set` takes them, with their labels.
pub const TEXT_JUSTIFY: &[(&str, &str)] = &[
    ("Left", "Left"),
    ("Center", "Center"),
    ("Right", "Right"),
    ("Align", "Aligned"),
    ("Middle", "Middle"),
    ("Fit", "Fit"),
    ("TL", "Top left"),
    ("TC", "Top center"),
    ("TR", "Top right"),
    ("ML", "Middle left"),
    ("MC", "Middle center"),
    ("MR", "Middle right"),
    ("BL", "Bottom left"),
    ("BC", "Bottom center"),
    ("BR", "Bottom right"),
];

/// MTEXT attachments 1–9 as `properties.set` takes them, with their labels.
pub const MTEXT_JUSTIFY: &[(&str, &str)] = &[
    ("TL", "Top left"),
    ("TC", "Top center"),
    ("TR", "Top right"),
    ("ML", "Middle left"),
    ("MC", "Middle center"),
    ("MR", "Middle right"),
    ("BL", "Bottom left"),
    ("BC", "Bottom center"),
    ("BR", "Bottom right"),
];

fn options(list: &[(&str, &str)]) -> Vec<Choice> {
    list.iter().map(|(v, l)| Choice::new(json!(v), *l)).collect()
}

/// The justification code of a TEXT (one of [`TEXT_JUSTIFY`]).
pub fn text_justify(t: &Text) -> &'static str {
    match (t.halign, t.valign) {
        (HAlign::Aligned, _) => "Align",
        (HAlign::Fit, _) => "Fit",
        (HAlign::Middle, _) => "Middle",
        (HAlign::Left, VAlign::Baseline) => "Left",
        (HAlign::Center, VAlign::Baseline) => "Center",
        (HAlign::Right, VAlign::Baseline) => "Right",
        (HAlign::Left, VAlign::Top) => "TL",
        (HAlign::Center, VAlign::Top) => "TC",
        (HAlign::Right, VAlign::Top) => "TR",
        (HAlign::Left, VAlign::Middle) => "ML",
        (HAlign::Center, VAlign::Middle) => "MC",
        (HAlign::Right, VAlign::Middle) => "MR",
        (HAlign::Left, VAlign::Bottom) => "BL",
        (HAlign::Center, VAlign::Bottom) => "BC",
        (HAlign::Right, VAlign::Bottom) => "BR",
    }
}

/// The justification code of an MTEXT attachment (1–9; anything else reads as top left).
pub fn mtext_justify(attach: u8) -> &'static str {
    MTEXT_JUSTIFY.get(usize::from(attach.max(1)) - 1).map_or("TL", |(code, _)| *code)
}

fn text_groups(c: &Ctx, ents: &[&Entity], texts: &[&Text]) -> Vec<Group> {
    let mut text = vec![
        c.row("text", "Contents", Kind::Text, texts.iter().map(|t| json!(t.value))).quick(),
        c.choice("style", "Style", Kind::TextStyle, c.text_styles(), texts.iter().map(|t| json!(t.style))).quick(),
        annotative(c, ents),
        c.choice("justify", "Justify", Kind::Choice, options(TEXT_JUSTIFY), texts.iter().map(|t| json!(text_justify(t)))).quick(),
        c.row("height", "Height", Kind::Distance, texts.iter().map(|t| json!(t.height))).quick(),
        c.row("rotation", "Rotation", Kind::Angle, texts.iter().map(|t| json!(t.rotation.to_degrees()))).quick(),
        c.row("widthFactor", "Width factor", Kind::Number, texts.iter().map(|t| json!(t.width_factor))),
        c.row("oblique", "Obliquing", Kind::Angle, texts.iter().map(|t| json!(t.oblique.to_degrees()))),
    ];
    // Aligned and fit text sit between two points: the second is the text alignment point.
    let second: Option<Vec<cadcraft_geom::Vec3>> =
        texts.iter().map(|t| if matches!(t.halign, HAlign::Aligned | HAlign::Fit) { t.align_pt } else { None }).collect();
    if let Some(pts) = second {
        text.push(c.row("alignmentX", "Text alignment X", Kind::Distance, pts.iter().map(|p| json!(p.x))));
        text.push(c.row("alignmentY", "Text alignment Y", Kind::Distance, pts.iter().map(|p| json!(p.y))));
    }
    // The position is the point the text is placed by (see `Text::justify_point`).
    let geometry = vec![
        c.row("positionX", "Position X", Kind::Distance, texts.iter().map(|t| json!(t.justify_point().x))),
        c.row("positionY", "Position Y", Kind::Distance, texts.iter().map(|t| json!(t.justify_point().y))),
        c.row("positionZ", "Position Z", Kind::Distance, texts.iter().map(|t| json!(t.insert.z))),
    ];
    vec![Group { name: "Text", rows: text }, Group { name: "Geometry", rows: geometry }]
}

fn mtext_groups(c: &Ctx, ents: &[&Entity], mts: &[&MText]) -> Vec<Group> {
    let spacing = vec![Choice::new(json!("atLeast"), "At least"), Choice::new(json!("exactly"), "Exactly")];
    let columns = |t: &MText| match &t.columns {
        None => "None".to_string(),
        Some(col) => match col.kind {
            cadcraft_doc::MTextColumnType::Static => format!("Static ({})", col.count.max(1)),
            cadcraft_doc::MTextColumnType::Dynamic => "Dynamic".to_string(),
        },
    };
    let text = vec![
        c.row("text", "Contents", Kind::Contents, mts.iter().map(|t| json!(t.contents))).quick(),
        c.choice("style", "Style", Kind::TextStyle, c.text_styles(), mts.iter().map(|t| json!(t.style))).quick(),
        annotative(c, ents),
        c.choice("justify", "Justify", Kind::Choice, options(MTEXT_JUSTIFY), mts.iter().map(|t| json!(mtext_justify(t.attach)))).quick(),
        c.row("height", "Text height", Kind::Distance, mts.iter().map(|t| json!(t.height))).quick(),
        c.row("rotation", "Rotation", Kind::Angle, mts.iter().map(|t| json!(t.rotation.to_degrees()))).quick(),
        c.row("lineSpacing", "Line space factor", Kind::Number, mts.iter().map(|t| json!(t.line_spacing))),
        c.row("lineSpacingDistance", "Line space distance", Kind::Distance, mts.iter().map(|t| json!(t.line_spacing * t.height * LINE_PITCH))),
        c.choice(
            "lineSpacingStyle",
            "Line space style",
            Kind::Choice,
            spacing,
            mts.iter().map(|t| json!(if t.line_spacing_exact { "exactly" } else { "atLeast" })),
        ),
        c.row("width", "Defined width", Kind::Distance, mts.iter().map(|t| json!(t.width))),
        c.row("columns", "Columns", Kind::Text, mts.iter().map(|t| json!(columns(t)))).read_only(),
    ];
    let geometry = vec![
        c.row("positionX", "Position X", Kind::Distance, mts.iter().map(|t| json!(t.insert.x))),
        c.row("positionY", "Position Y", Kind::Distance, mts.iter().map(|t| json!(t.insert.y))),
        c.row("positionZ", "Position Z", Kind::Distance, mts.iter().map(|t| json!(t.insert.z))),
    ];
    vec![Group { name: "Text", rows: text }, Group { name: "Geometry", rows: geometry }]
}

/// How a dimension style variable is entered.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DimPropKind {
    Distance,
    Number,
    /// Degrees.
    Angle,
    Bool,
    /// Numbers with labels.
    Choice(&'static [(i64, &'static str)]),
    /// Decimal places 0–8.
    Precision,
    Color,
    TextStyle,
    /// A linetype name; "" (and "ByBlock") is ByBlock.
    Linetype,
    /// Hundredths of a millimetre, or -1 ByLayer, -2 ByBlock, -3 Default.
    Lineweight,
    /// An arrowhead name ("" is closed filled).
    Arrow,
    /// The decimal separator.
    Separator,
}

/// A dimension style variable as a Properties row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DimProp {
    /// The [`DimStyle`] field (camelCase), which is also the `properties.set` key.
    pub field: &'static str,
    pub group: &'static str,
    pub label: &'static str,
    pub kind: DimPropKind,
    /// Shown in the Properties palette (the rest belong to the style dialog only).
    pub palette: bool,
}

const fn prop(field: &'static str, group: &'static str, label: &'static str, kind: DimPropKind) -> DimProp {
    DimProp { field, group, label, kind, palette: true }
}

const fn style_only(field: &'static str, group: &'static str, label: &'static str, kind: DimPropKind) -> DimProp {
    DimProp { field, group, label, kind, palette: false }
}

/// The groups dimension rows come in, in order (after Misc).
pub const DIM_GROUPS: &[&str] = &["Lines & Arrows", "Text", "Fit", "Primary Units", "Alternate Units", "Tolerances"];

const TEXT_POS_HOR: &[(i64, &str)] = &[(0, "Centered"), (1, "At Ext Line 1"), (2, "At Ext Line 2"), (3, "Over Ext Line 1"), (4, "Over Ext Line 2")];
const TEXT_POS_VERT: &[(i64, &str)] = &[(0, "Centered"), (1, "Above"), (2, "Outside"), (3, "JIS"), (4, "Below")];
const FIT: &[(i64, &str)] = &[(0, "Text and arrows"), (1, "Arrows"), (2, "Text"), (3, "Best fit")];
const TEXT_MOVE: &[(i64, &str)] = &[(0, "Keep dim line with text"), (1, "Move text, add leader"), (2, "Move text, no leader")];
const LINEAR_UNITS: &[(i64, &str)] =
    &[(1, "Scientific"), (2, "Decimal"), (3, "Engineering"), (4, "Architectural"), (5, "Fractional"), (6, "Windows desktop")];
const ALT_UNITS: &[(i64, &str)] = &[
    (1, "Scientific"),
    (2, "Decimal"),
    (3, "Engineering"),
    (4, "Architectural stacked"),
    (5, "Fractional stacked"),
    (6, "Architectural"),
    (7, "Fractional"),
    (8, "Windows desktop"),
];
const ANGLE_FORMAT: &[(i64, &str)] = &[(0, "Decimal degrees"), (1, "Degrees Minutes Seconds"), (2, "Gradians"), (3, "Radians")];
const FRACTION_FORMAT: &[(i64, &str)] = &[(0, "Horizontal"), (1, "Diagonal"), (2, "Not stacked")];
const TOL_POS: &[(i64, &str)] = &[(0, "Bottom"), (1, "Middle"), (2, "Top")];
const ARC_SYMBOL: &[(i64, &str)] = &[(0, "Before text"), (1, "Above text"), (2, "None")];

/// Every modeled dimension style variable as a row, grouped as AutoCAD's Properties palette
/// groups dimension properties. The rows not shown in the palette are style settings only.
pub const DIM_PROPS: &[DimProp] = &[
    // Lines & Arrows
    prop("arrowBlock1", "Lines & Arrows", "Arrow 1", DimPropKind::Arrow),
    prop("arrowBlock2", "Lines & Arrows", "Arrow 2", DimPropKind::Arrow),
    style_only("arrowBlock", "Lines & Arrows", "Arrowheads", DimPropKind::Arrow),
    prop("arrowSize", "Lines & Arrows", "Arrow size", DimPropKind::Distance),
    prop("tickSize", "Lines & Arrows", "Tick size", DimPropKind::Distance),
    prop("dimLineColor", "Lines & Arrows", "Dim line color", DimPropKind::Color),
    prop("dimLinetype", "Lines & Arrows", "Dim line linetype", DimPropKind::Linetype),
    prop("dimLineweight", "Lines & Arrows", "Dim line lineweight", DimPropKind::Lineweight),
    prop("dimLineExtend", "Lines & Arrows", "Dim line ext", DimPropKind::Distance),
    prop("suppressDim1", "Lines & Arrows", "Suppress dim line 1", DimPropKind::Bool),
    prop("suppressDim2", "Lines & Arrows", "Suppress dim line 2", DimPropKind::Bool),
    prop("extLineColor", "Lines & Arrows", "Ext line color", DimPropKind::Color),
    prop("extLinetype1", "Lines & Arrows", "Ext line 1 linetype", DimPropKind::Linetype),
    prop("extLinetype2", "Lines & Arrows", "Ext line 2 linetype", DimPropKind::Linetype),
    prop("extLineweight", "Lines & Arrows", "Ext line lineweight", DimPropKind::Lineweight),
    prop("extExtend", "Lines & Arrows", "Ext line ext", DimPropKind::Distance),
    prop("extOffset", "Lines & Arrows", "Ext line offset", DimPropKind::Distance),
    prop("fixedExt", "Lines & Arrows", "Ext line fixed", DimPropKind::Bool),
    prop("fixedExtLength", "Lines & Arrows", "Ext line fixed length", DimPropKind::Distance),
    prop("suppressExt1", "Lines & Arrows", "Suppress ext line 1", DimPropKind::Bool),
    prop("suppressExt2", "Lines & Arrows", "Suppress ext line 2", DimPropKind::Bool),
    prop("centerMark", "Lines & Arrows", "Center mark size", DimPropKind::Distance),
    prop("jogAngle", "Lines & Arrows", "Jog angle", DimPropKind::Angle),
    prop("arcSymbol", "Lines & Arrows", "Arc length symbol", DimPropKind::Choice(ARC_SYMBOL)),
    style_only("leaderArrowBlock", "Lines & Arrows", "Leader arrow", DimPropKind::Arrow),
    style_only("baselineSpacing", "Lines & Arrows", "Baseline spacing", DimPropKind::Distance),
    // Text
    prop("textColor", "Text", "Text color", DimPropKind::Color),
    prop("textHeight", "Text", "Text height", DimPropKind::Distance),
    prop("textGap", "Text", "Text offset", DimPropKind::Distance),
    prop("textOutsideHorizontal", "Text", "Text outside align", DimPropKind::Bool),
    prop("textJust", "Text", "Text pos hor", DimPropKind::Choice(TEXT_POS_HOR)),
    prop("textAbove", "Text", "Text pos vert", DimPropKind::Choice(TEXT_POS_VERT)),
    prop("textStyle", "Text", "Text style", DimPropKind::TextStyle),
    prop("textInsideHorizontal", "Text", "Text inside align", DimPropKind::Bool),
    style_only("textVerticalPos", "Text", "Text vertical position", DimPropKind::Number),
    // Fit
    prop("forceLineInside", "Fit", "Dim line forced", DimPropKind::Bool),
    prop("suppressOutsideArrows", "Fit", "Suppress arrows outside", DimPropKind::Bool),
    prop("scale", "Fit", "Dim scale overall", DimPropKind::Number),
    prop("fit", "Fit", "Fit", DimPropKind::Choice(FIT)),
    prop("textInside", "Fit", "Text inside", DimPropKind::Bool),
    prop("textMove", "Fit", "Text movement", DimPropKind::Choice(TEXT_MOVE)),
    // Primary Units
    prop("decimalSeparator", "Primary Units", "Decimal separator", DimPropKind::Separator),
    prop("round", "Primary Units", "Dim roundoff", DimPropKind::Distance),
    prop("linearFactor", "Primary Units", "Dim scale linear", DimPropKind::Number),
    prop("linearUnit", "Primary Units", "Dim units", DimPropKind::Choice(LINEAR_UNITS)),
    prop("decimals", "Primary Units", "Precision", DimPropKind::Precision),
    prop("fractionFormat", "Primary Units", "Fraction format", DimPropKind::Choice(FRACTION_FORMAT)),
    prop("angularUnit", "Primary Units", "Angle format", DimPropKind::Choice(ANGLE_FORMAT)),
    prop("angularDecimals", "Primary Units", "Angle precision", DimPropKind::Precision),
    style_only("post", "Primary Units", "Prefix and suffix", DimPropKind::Number),
    style_only("zeroSuppression", "Primary Units", "Zero suppression", DimPropKind::Number),
    style_only("angularZeroSuppression", "Primary Units", "Angular zero suppression", DimPropKind::Number),
    // Alternate Units
    prop("alt", "Alternate Units", "Alt enabled", DimPropKind::Bool),
    prop("altUnit", "Alternate Units", "Alt format", DimPropKind::Choice(ALT_UNITS)),
    prop("altDecimals", "Alternate Units", "Alt precision", DimPropKind::Precision),
    prop("altFactor", "Alternate Units", "Alt scale factor", DimPropKind::Number),
    prop("altRound", "Alternate Units", "Alt round", DimPropKind::Number),
    style_only("altPost", "Alternate Units", "Alt prefix and suffix", DimPropKind::Number),
    style_only("altZeroSuppression", "Alternate Units", "Alt zero suppression", DimPropKind::Number),
    // Tolerances
    prop("tolerance", "Tolerances", "Tolerances", DimPropKind::Bool),
    prop("limits", "Tolerances", "Limits", DimPropKind::Bool),
    prop("tolPlus", "Tolerances", "Tolerance limit upper", DimPropKind::Number),
    prop("tolMinus", "Tolerances", "Tolerance limit lower", DimPropKind::Number),
    prop("tolDecimals", "Tolerances", "Tolerance precision", DimPropKind::Precision),
    prop("tolScale", "Tolerances", "Tolerance text height", DimPropKind::Number),
    prop("tolJust", "Tolerances", "Tolerance pos vert", DimPropKind::Choice(TOL_POS)),
    style_only("tolZeroSuppression", "Tolerances", "Tolerance zero suppression", DimPropKind::Number),
];

/// The arrowheads drawn in code, by the DIMBLK name stored for them and their label.
pub const ARROWHEADS: &[(&str, &str)] = &[
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
    ("_BOXBLANK", "Box blank"),
    ("_BOXFILLED", "Box filled"),
    ("_DATUMBLANK", "Datum triangle"),
    ("_DATUMFILLED", "Datum triangle filled"),
    ("_INTEGRAL", "Integral"),
    ("_NONE", "None"),
];

/// The row for dimension style field `field`, if it has one.
pub fn dim_prop(field: &str) -> Option<&'static DimProp> {
    DIM_PROPS.iter().find(|p| p.field.eq_ignore_ascii_case(field))
}

/// The dimension style fields whose value is a linetype name ("" is ByBlock).
pub fn is_dim_linetype(field: &str) -> bool {
    matches!(field, "dimLinetype" | "extLinetype1" | "extLinetype2")
}

impl DimPropKind {
    fn row_kind(self, c: &Ctx) -> (Kind, Vec<Choice>) {
        match self {
            DimPropKind::Distance => (Kind::Distance, Vec::new()),
            DimPropKind::Number => (Kind::Number, Vec::new()),
            DimPropKind::Angle => (Kind::Angle, Vec::new()),
            DimPropKind::Bool => (Kind::Bool, Vec::new()),
            DimPropKind::Choice(list) => (Kind::Choice, list.iter().map(|(v, l)| Choice::new(json!(v), *l)).collect()),
            DimPropKind::Precision => (
                Kind::Choice,
                (0..=8u8).map(|n| Choice::new(json!(n), if n == 0 { "0".into() } else { format!("0.{}", "0".repeat(usize::from(n))) })).collect(),
            ),
            DimPropKind::Color => (Kind::Color, Vec::new()),
            DimPropKind::TextStyle => (Kind::TextStyle, c.text_styles()),
            DimPropKind::Linetype => (Kind::Linetype, Vec::new()),
            DimPropKind::Lineweight => {
                let mut o = vec![Choice::new(json!(-2), "ByBlock"), Choice::new(json!(-1), "ByLayer"), Choice::new(json!(-3), "Default")];
                o.extend(Lineweight::STANDARD.iter().map(|w| Choice::new(json!(w), Lineweight::Mm100(*w).name())));
                (Kind::Choice, o)
            }
            DimPropKind::Arrow => (Kind::Choice, ARROWHEADS.iter().map(|(v, l)| Choice::new(json!(v), *l)).collect()),
            DimPropKind::Separator => {
                (Kind::Choice, [(".", "Period"), (",", "Comma"), (" ", "Space")].iter().map(|(v, l)| Choice::new(json!(v), *l)).collect())
            }
        }
    }
}

/// `name` normalized as the renderer reads arrowhead names ("_DOT", "dot" and "Dot" alike).
fn arrow_key(name: &str) -> String {
    name.trim().trim_start_matches('_').chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_lowercase()
}

/// A dimension style field's value as the row shows and `properties.set` takes it.
fn dim_value(p: &DimProp, st: &Value) -> Value {
    let v = st.get(p.field).cloned().unwrap_or(Value::Null);
    match p.kind {
        DimPropKind::Color => serde_json::from_value::<cadcraft_color::Color>(v).map(|c| json!(c.name())).unwrap_or(Value::Null),
        DimPropKind::Linetype => match v.as_str() {
            Some("") | None => json!("ByBlock"),
            Some(_) => v,
        },
        DimPropKind::Arrow => {
            // Per-end arrowheads left empty use DIMBLK.
            let name = v.as_str().filter(|s| !s.is_empty() || p.field == "arrowBlock").map(str::to_string);
            let name = name.unwrap_or_else(|| st.get("arrowBlock").and_then(Value::as_str).unwrap_or_default().to_string());
            // A drawn arrowhead reads as its standard name; a block of the user's stays as it is.
            match ARROWHEADS.iter().find(|(n, _)| arrow_key(n) == arrow_key(&name) || (n.is_empty() && arrow_key(&name) == "closedfilled")) {
                Some((n, _)) => json!(n),
                None => json!(name),
            }
        }
        _ => v,
    }
}

/// Whether dimension `dm` overrides style field `field`.
fn overrides(dm: &Dimension, field: &str) -> bool {
    dm.overrides.keys().any(|k| DimStyle::field_name(k) == Some(field))
}

/// The prefix and suffix of a DIMPOST/DIMAPOST template: around `<>`, or the whole template
/// as a suffix when it has no `<>` (as the renderer reads it).
pub fn split_post(template: &str) -> (String, String) {
    match template.split_once("<>") {
        Some((pre, post)) => (pre.to_string(), post.to_string()),
        None => (String::new(), template.to_string()),
    }
}

/// A DIMPOST/DIMAPOST template from a prefix and a suffix.
pub fn join_post(prefix: &str, suffix: &str) -> String {
    if prefix.is_empty() { suffix.to_string() } else { format!("{prefix}<>{suffix}") }
}

/// The measurement as the dimension writes it (its own units and precision).
fn measurement_text(dm: &Dimension, st: &DimStyle, meas: f64) -> String {
    match dm.kind {
        DimKind::Angular | DimKind::Angular3P => format!("{:.*}°", usize::from(st.angular_decimals.min(8)), meas.to_degrees()),
        _ => cadcraft_render::format_linear_value(meas, st),
    }
}

fn dimension_groups(c: &Ctx, ents: &[&Entity], dims: &[&Dimension]) -> Vec<Group> {
    let styles: Vec<DimStyle> = dims.iter().map(|dm| c.d.dim_style(&dm.style).cloned().unwrap_or_default().with_overrides(&dm.overrides)).collect();
    let values: Vec<Value> = styles.iter().map(|st| serde_json::to_value(st).unwrap_or(Value::Null)).collect();
    // Measurement and text position need each dimension's geometry.
    let geometry: Option<Vec<cadcraft_render::DimGeometry>> =
        (dims.len() <= GEOMETRY_CAP).then(|| dims.iter().map(|dm| cadcraft_render::dimension_in(c.d, dm)).collect());
    let mut groups = vec![Group {
        name: "Misc",
        rows: vec![
            c.choice("dimStyle", "Dim style", Kind::DimStyle, c.dim_styles(), dims.iter().map(|dm| json!(dm.style))).quick(),
            annotative(c, ents),
        ],
    }];
    for &name in DIM_GROUPS {
        let mut rows: Vec<Row> = Vec::new();
        for p in DIM_PROPS.iter().filter(|p| p.group == name && p.palette) {
            let (kind, options) = p.kind.row_kind(c);
            let mut r = c.choice(p.field, p.label, kind, options, values.iter().map(|v| dim_value(p, v)));
            r.overridden = dims.iter().any(|dm| overrides(dm, p.field));
            if p.field == "textHeight" {
                fixed_height(c, &mut r, &styles);
            }
            rows.push(r);
        }
        match name {
            "Text" => {
                let (xs, ys): (Vec<Value>, Vec<Value>) = match &geometry {
                    Some(g) => g.iter().map(|g| (json!(g.text_pos.x), json!(g.text_pos.y))).unzip(),
                    None => (vec![Value::Null, json!(VARIES)], vec![Value::Null, json!(VARIES)]),
                };
                rows.push(c.row("textPositionX", "Text position X", Kind::Distance, xs));
                rows.push(c.row("textPositionY", "Text position Y", Kind::Distance, ys));
                rows.push(c.row("textRotation", "Text rotation", Kind::Angle, dims.iter().map(|dm| json!(dm.text_rotation.to_degrees()))));
                let measured: Vec<(Value, String)> = match &geometry {
                    Some(g) => g
                        .iter()
                        .zip(dims)
                        .zip(&styles)
                        .map(|((g, dm), st)| {
                            let angular = matches!(dm.kind, DimKind::Angular | DimKind::Angular3P);
                            (json!(if angular { g.measurement.to_degrees() } else { g.measurement }), measurement_text(dm, st, g.measurement))
                        })
                        .collect(),
                    None => vec![(Value::Null, String::new()), (json!(VARIES), String::new())],
                };
                // A length, or an angle in degrees, shown as the dimension writes it.
                let mut m = c.row("measurement", "Measurement", Kind::Number, measured.iter().map(|(v, _)| v.clone())).read_only().quick();
                if !m.varies {
                    m.display = measured.first().map(|(_, s)| s.clone()).unwrap_or_default();
                }
                rows.push(m);
                rows.push(c.row("textOverride", "Text override", Kind::Text, dims.iter().map(|dm| json!(dm.text))).quick());
            }
            "Primary Units" => {
                let (pre, suf): (Vec<Value>, Vec<Value>) = styles.iter().map(|st| split_post(&st.post)).map(|(p, s)| (json!(p), json!(s))).unzip();
                let at = rows.iter().position(|r| r.key == "round").unwrap_or(rows.len());
                rows.insert(at, c.row("prefix", "Dim prefix", Kind::Text, pre));
                let at = rows.iter().position(|r| r.key == "linearUnit").unwrap_or(rows.len());
                rows.insert(at, c.row("suffix", "Dim suffix", Kind::Text, suf));
            }
            "Alternate Units" => {
                let (pre, suf): (Vec<Value>, Vec<Value>) =
                    styles.iter().map(|st| split_post(&st.alt_post)).map(|(p, s)| (json!(p), json!(s))).unzip();
                rows.push(c.row("altPrefix", "Alt prefix", Kind::Text, pre));
                rows.push(c.row("altSuffix", "Alt suffix", Kind::Text, suf));
            }
            _ => {}
        }
        groups.push(Group { name, rows });
    }
    groups
}

/// A fixed-height text style decides the dimension text height whatever DIMTXT says (as in
/// AutoCAD); the row says so instead of taking edits that change nothing.
fn fixed_height(c: &Ctx, r: &mut Row, styles: &[DimStyle]) {
    let fixed: Vec<Option<(&str, f64)>> = styles
        .iter()
        .map(|st| c.d.text_style(&st.text_style).filter(|ts| ts.height > 0.0 && ts.height.is_finite()).map(|ts| (ts.name.as_str(), ts.height)))
        .collect();
    let Some(&Some((name, h))) = fixed.iter().find(|f| f.is_some()) else { return };
    if fixed.iter().all(|f| *f == Some((name, h))) {
        r.value = json!(h);
        r.varies = false;
        r.display = c.distance(h);
        r.editable = false;
        r.note =
            Some(format!("Set by text style \"{name}\", which has a fixed height. Give the style a height of 0 to use the dimension text height."));
    } else {
        r.editable = fixed.iter().any(Option::is_none);
        r.note = Some("Some of these dimensions take their text height from a fixed-height text style.".into());
    }
}
