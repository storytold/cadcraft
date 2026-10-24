//! Entities: the graphical objects of a drawing.

use cadcraft_color::Color;
use cadcraft_geom::{
    Arc as GArc, Bounds2, Circle as GCircle, Ellipse as GEllipse, Line as GLine, Mat3, PolyVertex, Polyline, Segment, Spline as GSpline, Vec2, Vec3,
};
use serde::{Deserialize, Serialize};

mod mline;
pub use mline::*;

/// A database handle (DXF group 5), unique per drawing.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Handle(pub u64);

impl Handle {
    pub fn hex(self) -> String {
        format!("{:X}", self.0)
    }
    pub fn parse_hex(s: &str) -> Option<Handle> {
        let s = s.trim();
        if !s.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        u64::from_str_radix(s, 16).ok().map(Handle)
    }
}

/// Lineweight in hundredths of a millimetre, or a logical value.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Lineweight {
    #[default]
    ByLayer,
    ByBlock,
    Default,
    /// 0..=211 (hundredths of mm).
    Mm100(u16),
}

impl Lineweight {
    /// The standard lineweight values (mm × 100).
    pub const STANDARD: [u16; 24] = [0, 5, 9, 13, 15, 18, 20, 25, 30, 35, 40, 50, 53, 60, 70, 80, 90, 100, 106, 120, 140, 158, 200, 211];
    pub fn to_dxf(self) -> i16 {
        match self {
            Lineweight::ByLayer => -1,
            Lineweight::ByBlock => -2,
            Lineweight::Default => -3,
            Lineweight::Mm100(v) => v.min(211) as i16,
        }
    }
    pub fn from_dxf(v: i16) -> Lineweight {
        match v {
            -2 => Lineweight::ByBlock,
            -3 => Lineweight::Default,
            0..=211 => Lineweight::Mm100(v as u16),
            _ => Lineweight::ByLayer,
        }
    }
    pub fn name(self) -> String {
        match self {
            Lineweight::ByLayer => "ByLayer".into(),
            Lineweight::ByBlock => "ByBlock".into(),
            Lineweight::Default => "Default".into(),
            Lineweight::Mm100(v) => format!("{:.2} mm", f64::from(v) / 100.0),
        }
    }
}

/// Transparency: ByLayer, ByBlock, or 0..=90 percent.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Transparency {
    #[default]
    ByLayer,
    ByBlock,
    Percent(u8),
}

impl Transparency {
    /// "ByLayer", "ByBlock" or the percentage ("50").
    pub fn name(&self) -> String {
        match self {
            Transparency::ByLayer => "ByLayer".into(),
            Transparency::ByBlock => "ByBlock".into(),
            Transparency::Percent(p) => p.to_string(),
        }
    }
}

/// Properties every entity has.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Common {
    pub layer: String,
    pub color: Color,
    pub linetype: String,
    pub lineweight: Lineweight,
    pub ltscale: f64,
    pub transparency: Transparency,
    pub visible: bool,
    pub thickness: f64,
    pub extrusion: Vec3,
    /// Named plot style: `ByLayer`, `ByBlock` or a style of the layout's named (`.stb`) plot
    /// style table.
    pub plot_style: String,
    /// The annotation scales an annotative object supports (empty: not annotative).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub anno_scales: Vec<crate::AnnoScale>,
    /// Other applications' extended data (xdata) as read from the drawing file: whole
    /// applications, each from its `1001` group on. It is part of the object, so a copy of the
    /// object (COPY, MIRROR, ARRAY, paste) carries it, as in AutoCAD.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub xdata: Vec<RawTag>,
}

impl Default for Common {
    fn default() -> Self {
        Common {
            layer: "0".into(),
            color: Color::ByLayer,
            linetype: "ByLayer".into(),
            lineweight: Lineweight::ByLayer,
            ltscale: 1.0,
            transparency: Transparency::ByLayer,
            visible: true,
            thickness: 0.0,
            extrusion: Vec3::Z,
            plot_style: "ByLayer".into(),
            anno_scales: Vec::new(),
            xdata: Vec::new(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Entity {
    pub handle: Handle,
    #[serde(default)]
    pub common: Common,
    pub kind: EntityKind,
}

impl Entity {
    pub fn new(handle: Handle, kind: EntityKind) -> Self {
        Entity { handle, common: Common::default(), kind }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Line {
    pub a: Vec3,
    pub b: Vec3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Point {
    pub p: Vec3,
    #[serde(default)]
    pub angle: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Circle {
    pub center: Vec3,
    pub radius: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Arc {
    pub center: Vec3,
    pub radius: f64,
    /// Radians, CCW.
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Ellipse {
    pub center: Vec3,
    pub major: Vec3,
    pub ratio: f64,
    pub start: f64,
    pub end: f64,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LwPolyline {
    pub vertices: Vec<PolyVertex>,
    pub closed: bool,
    #[serde(default)]
    pub const_width: f64,
    #[serde(default)]
    pub elevation: f64,
    /// Polyline linetype generation (PLINEGEN).
    #[serde(default)]
    pub plinegen: bool,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Polyline3d {
    pub points: Vec<Vec3>,
    pub closed: bool,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum HAlign {
    #[default]
    Left,
    Center,
    Right,
    Aligned,
    Middle,
    Fit,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum VAlign {
    #[default]
    Baseline,
    Bottom,
    Middle,
    Top,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Text {
    pub insert: Vec3,
    #[serde(default)]
    pub align_pt: Option<Vec3>,
    pub height: f64,
    pub value: String,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "one")]
    pub width_factor: f64,
    #[serde(default)]
    pub oblique: f64,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default)]
    pub halign: HAlign,
    #[serde(default)]
    pub valign: VAlign,
}
impl Text {
    /// The point the text is placed by, as the Insertion snap and the grip show it: the alignment
    /// point (DXF group 11) of justified text, the start point (group 10) of left-baseline,
    /// aligned and fit text.
    pub fn justify_point(&self) -> Vec2 {
        match (self.halign, self.valign) {
            (HAlign::Left, VAlign::Baseline) | (HAlign::Aligned | HAlign::Fit, _) => self.insert.xy(),
            _ => self.align_pt.unwrap_or(self.insert).xy(),
        }
    }
}
fn one() -> f64 {
    1.0
}
fn standard() -> String {
    "Standard".into()
}

mod helix;
pub use helix::*;
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MText {
    pub insert: Vec3,
    pub height: f64,
    /// Reference rectangle width (0 = no wrapping).
    #[serde(default)]
    pub width: f64,
    /// 1..=9: TL TC TR ML MC MR BL BC BR.
    #[serde(default = "attach_tl")]
    pub attach: u8,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default = "standard")]
    pub style: String,
    /// Raw MTEXT contents with inline codes (\P, {\fArial|b1;...}, \S1/2; …).
    pub contents: String,
    #[serde(default = "one")]
    pub line_spacing: f64,
    /// Line spacing style (DXF 73): Exactly (`true`) keeps `line_spacing` whatever the character
    /// heights; At least (`false`, the default) lets taller characters push lines apart.
    #[serde(default)]
    pub line_spacing_exact: bool,
    /// Columns the text flows through, each `width` wide (DXF 75/76/78/79/48/49/50); `None`:
    /// one column.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub columns: Option<MTextColumns>,
}
fn attach_tl() -> u8 {
    1
}

/// Static columns: a fixed number of columns of one height. Dynamic columns: as many columns
/// as the text needs, of one height (auto height) or each its own (manual heights).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MTextColumnType {
    #[default]
    Static,
    Dynamic,
}

/// MTEXT columns (MTEXT Columns option, DXF Reference MTEXT groups 75–79, 48–50).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct MTextColumns {
    #[serde(rename = "type")]
    pub kind: MTextColumnType,
    /// Number of columns (static columns: the columns there are).
    pub count: u16,
    /// Space between two columns.
    pub gutter: f64,
    /// Height of every column (static, and dynamic with auto height).
    pub height: f64,
    /// Dynamic columns: one height for all columns (`height`) instead of `heights`.
    pub auto_height: bool,
    /// Dynamic columns with manual heights: the height of each column.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub heights: Vec<f64>,
    /// Columns run right to left.
    pub flow_reversed: bool,
}

impl MTextColumns {
    /// The width of `count` columns of `column_width` and their gutters.
    pub fn total_width(&self, column_width: f64) -> f64 {
        let n = f64::from(self.count.max(1));
        column_width * n + self.gutter.max(0.0) * (n - 1.0)
    }

    /// The tallest column.
    pub fn max_height(&self) -> f64 {
        self.heights.iter().copied().fold(self.height, f64::max).max(0.0)
    }

    /// Scale every length (SCALE, block insertion).
    pub fn scale(&mut self, s: f64) {
        self.gutter *= s;
        self.height *= s;
        self.heights.iter_mut().for_each(|h| *h *= s);
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Attrib {
    pub tag: String,
    pub text: Text,
    #[serde(default)]
    pub invisible: bool,
    #[serde(default)]
    pub constant: bool,
    #[serde(default)]
    pub prompt: String,
    /// An attribute's own properties; each one left unset follows the attribute definition.
    #[serde(default, skip_serializing_if = "AttribProps::is_empty")]
    pub props: AttribProps,
    /// A multiline attribute (DXF attribute type 2, or 4 for a definition): its text as
    /// multiline text. `text.value` holds the plain text, which value edits change.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub multiline: Option<MultilineAttrib>,
    /// Its other modes: Verify, Preset and Lock position.
    #[serde(default, skip_serializing_if = "AttribModes::is_default")]
    pub modes: AttribModes,
}

/// An attribute's modes beyond invisible and constant.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AttribModes {
    /// Verify: the value is asked for twice when the block is inserted (DXF flag 4).
    #[serde(default)]
    pub verify: bool,
    /// Preset: the default value is used without asking (DXF flag 8).
    #[serde(default)]
    pub preset: bool,
    /// Lock position: the attribute can't be moved on its own within a reference (DXF group
    /// 280 after the flags). A reference's attributes take it from their definitions.
    #[serde(default)]
    pub lock_position: bool,
}

impl AttribModes {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
    /// The modes of a reference's attribute made from a definition with these: AutoCAD keeps
    /// only Lock position there.
    pub fn for_reference(self) -> Self {
        AttribModes { lock_position: self.lock_position, ..Self::default() }
    }
}

/// The multiline text of a multiline attribute.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MultilineAttrib {
    pub mtext: MText,
    /// The plain text `mtext`'s contents read as: an attribute whose value differs was edited
    /// since, and its value replaces the contents.
    pub read_as: String,
}

impl MultilineAttrib {
    /// This text placed by `m` (a block reference's transform, for its attribute made from a
    /// definition).
    pub fn transformed(&self, m: &Mat3) -> MultilineAttrib {
        let mut k = EntityKind::MText(self.mtext.clone());
        k.transform(m);
        let mtext = if let EntityKind::MText(t) = k { t } else { self.mtext.clone() };
        MultilineAttrib { mtext, read_as: self.read_as.clone() }
    }
}

impl Attrib {
    /// Move a multiline attribute's text by `m` along with the attribute (nothing for others).
    pub fn transform_multiline(&mut self, m: &Mat3) {
        if let Some(ml) = &mut self.multiline {
            *ml = ml.transformed(m);
        }
    }

    /// A multiline attribute's text as drawn and saved: its multiline text, with the contents
    /// replaced by the value when the value was edited since it was read.
    pub fn multiline_text(&self) -> Option<MText> {
        let m = self.multiline.as_ref()?;
        let mut t = m.mtext.clone();
        if self.text.value != m.read_as {
            t.contents = mtext_literal(&self.text.value);
        }
        Some(t)
    }
}

/// `s` as MTEXT contents that read back as `s`: backslashes and braces escaped, line breaks as
/// paragraph breaks.
pub fn mtext_literal(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '{' => out.push_str("\\{"),
            '}' => out.push_str("\\}"),
            '\n' => out.push_str("\\P"),
            '\r' => {}
            c => out.push(c),
        }
    }
    out
}

/// Properties set on one attribute of a block reference (an ATTRIB's layer, colour, linetype and
/// lineweight), overriding those of its definition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AttribProps {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<Color>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linetype: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lineweight: Option<Lineweight>,
}

impl AttribProps {
    /// Nothing set: the attribute is drawn with its definition's properties.
    pub fn is_empty(&self) -> bool {
        *self == AttribProps::default()
    }
    /// `base` (the definition's properties) with the ones set here in place.
    pub fn over(&self, base: &Common) -> Common {
        Common {
            layer: self.layer.clone().unwrap_or_else(|| base.layer.clone()),
            color: self.color.unwrap_or(base.color),
            linetype: self.linetype.clone().unwrap_or_else(|| base.linetype.clone()),
            lineweight: self.lineweight.unwrap_or(base.lineweight),
            ..base.clone()
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Insert {
    pub block: String,
    pub insert: Vec3,
    #[serde(default = "unit_scale")]
    pub scale: Vec3,
    #[serde(default)]
    pub rotation: f64,
    #[serde(default)]
    pub attribs: Vec<Attrib>,
    #[serde(default = "one_u")]
    pub cols: u32,
    #[serde(default = "one_u")]
    pub rows: u32,
    #[serde(default)]
    pub col_spacing: f64,
    #[serde(default)]
    pub row_spacing: f64,
}
fn unit_scale() -> Vec3 {
    Vec3::new(1.0, 1.0, 1.0)
}
fn one_u() -> u32 {
    1
}
impl Insert {
    /// Block-to-world transform (2D part).
    pub fn transform(&self, base: Vec2) -> Mat3 {
        Mat3::translate(self.insert.xy())
            .then_before(Mat3::rotate(self.rotation))
            .then_before(Mat3::scale(self.scale.x, self.scale.y))
            .then_before(Mat3::translate(-base))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DimKind {
    /// Rotated linear dimension (horizontal = 0, vertical = 90°).
    Linear {
        rotation: f64,
    },
    Aligned,
    Angular,
    Angular3P,
    Diameter,
    Radius,
    Ordinate {
        x_type: bool,
    },
    ArcLength,
}
/// A dimension. Definition points follow DXF: `defpt` (10: dimension line point),
/// `p13`/`p14` (extension origins), `p15`/`p16` (arc/angle points), `text_mid` (11).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Dimension {
    pub kind: DimKind,
    pub defpt: Vec3,
    pub text_mid: Vec3,
    #[serde(default)]
    pub p13: Vec3,
    #[serde(default)]
    pub p14: Vec3,
    #[serde(default)]
    pub p15: Vec3,
    #[serde(default)]
    pub p16: Vec3,
    /// "" = measured value, "<>" inside = measurement placeholder, " " = suppressed.
    #[serde(default)]
    pub text: String,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default)]
    pub measurement: f64,
    #[serde(default)]
    pub text_rotation: f64,
    /// User moved the text (text_mid is authoritative).
    #[serde(default)]
    pub user_text_pos: bool,
    /// Anonymous block with the rendered geometry (from files); regenerated when edited.
    #[serde(default)]
    pub block: Option<String>,
    /// Per-dimension style overrides (DIMOVERRIDE): DimStyle field names (camelCase) → values,
    /// merged over the named style.
    #[serde(default, skip_serializing_if = "serde_json::Map::is_empty")]
    pub overrides: serde_json::Map<String, serde_json::Value>,
    /// Associativity: which definition points follow which objects.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub assoc: Vec<DimAssoc>,
    /// Oblique extension lines, jogs, breaks and inspection frames.
    #[serde(default, skip_serializing_if = "DimMarks::is_empty")]
    pub marks: DimMarks,
}

/// What dimension edits add to a dimension's drawing: DIMEDIT Oblique, DIMJOGLINE, DIMJOGGED,
/// DIMBREAK and DIMINSPECT.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DimMarks {
    /// Angle (radians) of the extension lines of a linear or aligned dimension; None =
    /// perpendicular to the dimension line.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub oblique: Option<f64>,
    /// A jog on the dimension line of a linear or aligned dimension, at this point (DIMJOGLINE).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jog_line: Option<Vec3>,
    /// A jogged radius dimension (DIMJOGGED).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub jogged: Option<JoggedRadius>,
    /// Gaps in the dimension and extension lines (DIMBREAK).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub breaks: Vec<DimBreak>,
    /// Inspection frame, label and rate (DIMINSPECT).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub inspect: Option<Inspection>,
}

impl DimMarks {
    pub fn is_empty(&self) -> bool {
        *self == DimMarks::default()
    }

    /// Transform the points and angles (part of [`EntityKind::transform`]).
    fn transform(&mut self, t3: &dyn Fn(&mut Vec3), rot: f64, s: f64, mirror: bool) {
        if let Some(a) = &mut self.oblique {
            *a = cadcraft_geom::norm_angle(if mirror { rot - (*a - rot) } else { *a + rot });
        }
        if let Some(j) = &mut self.jog_line {
            t3(j);
        }
        if let Some(j) = &mut self.jogged {
            t3(&mut j.center);
            t3(&mut j.jog);
        }
        for b in &mut self.breaks {
            t3(&mut b.at);
            b.size *= s;
        }
    }
}

/// The definition of a jogged radius dimension: the dimension line starts at an overriding
/// centre and jogs on its way to the arc.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct JoggedRadius {
    /// Where the dimension line starts instead of the true centre.
    pub center: Vec3,
    /// The middle of the jog.
    pub jog: Vec3,
}

/// A gap in a dimension's lines: everything within `size / 2` of `at` is left out.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DimBreak {
    pub at: Vec3,
    pub size: f64,
    /// Made by DIMBREAK Auto (recomputed by the next Auto) rather than picked by hand.
    pub auto: bool,
}

/// An inspection dimension (DIMINSPECT).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Inspection {
    /// Frame shape: 0 round ends, 1 angular ends, 2 none.
    pub shape: u8,
    /// Text left of the value ("" = none).
    pub label: String,
    /// Inspection rate right of the value ("" = none).
    pub rate: String,
}

/// A dimension definition point that follows an object (associative dimensions).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct DimAssoc {
    /// Definition point: "defpt", "p13", "p14", "p15" or "p16".
    pub point: String,
    /// The object it is attached to.
    pub handle: Handle,
    pub snap: AssocSnap,
}

/// Where on the object an associative definition point sits.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AssocSnap {
    /// Start point of a line/arc (or polyline vertex 0).
    Start,
    /// End point of a line/arc.
    End,
    Mid,
    /// Centre of a circle/arc.
    Center,
    /// On a circle/arc at a fixed angle (radians) from its centre.
    OnCircle {
        angle: f64,
    },
    /// Intersection of this (line) object with another line.
    Intersection {
        other: Handle,
    },
    /// Vertex `index` of a polyline.
    Vertex {
        index: usize,
    },
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Leader {
    pub vertices: Vec<Vec3>,
    #[serde(default = "yes")]
    pub arrow: bool,
    #[serde(default)]
    pub spline: bool,
    #[serde(default = "standard")]
    pub style: String,
}
fn yes() -> bool {
    true
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MLeader {
    pub leaders: Vec<Vec<Vec3>>,
    pub landing: Vec3,
    pub dogleg: f64,
    pub text: Option<MText>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "one")]
    pub arrow_size: f64,
    /// Leader lines are smooth curves through their points (leader type Spline). An empty
    /// `leaders` is a multileader without a leader line (leader type None).
    #[serde(default)]
    pub spline: bool,
    /// Block content in place of the text: a block reference placed at the landing.
    #[serde(default)]
    pub block: Option<Insert>,
}

impl MLeader {
    /// Each leader line as drawn: through its points to the landing, smoothed for spline leaders.
    pub fn leader_paths(&self) -> Vec<Vec<Vec2>> {
        self.leaders
            .iter()
            .map(|l| {
                let pts: Vec<Vec2> = l.iter().map(|v| v.xy()).chain(std::iter::once(self.landing.xy())).collect();
                if self.spline && pts.len() > 2 { GSpline::from_fit_points(&pts).tessellate(1e-3) } else { pts }
            })
            .collect()
    }
}
/// A hatch boundary loop: closed polyline with bulges.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct HatchLoop {
    pub vertices: Vec<PolyVertex>,
    #[serde(default)]
    pub outer: bool,
    /// The objects the loop was made from (DXF group 97 count, 330 handles): an associative
    /// hatch re-fills the loop from them when they change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<Handle>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Hatch {
    pub pattern: String,
    pub solid: bool,
    pub loops: Vec<HatchLoop>,
    #[serde(default = "one")]
    pub scale: f64,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub associative: bool,
    /// 0 normal (odd parity), 1 outer, 2 ignore.
    #[serde(default)]
    pub style: u8,
    #[serde(default)]
    pub elevation: f64,
    /// Gradient fill: name + second colour.
    #[serde(default)]
    pub gradient: Option<Gradient>,
    #[serde(default)]
    pub origin: Vec2,
    #[serde(default)]
    pub background: Option<Color>,
    /// The pattern's line families as a file defined them, for a pattern that isn't in the
    /// standard library (unit scale and angle, like the library's; empty otherwise).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pattern_lines: Vec<crate::library::PatternLine>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Gradient {
    pub name: String,
    pub color1: Color,
    pub color2: Color,
    #[serde(default)]
    pub angle: f64,
    #[serde(default)]
    pub centered: bool,
}
/// SOLID / TRACE: up to four corners (DXF order 1,2,4,3 when drawn).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Solid {
    pub corners: [Vec3; 4],
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Face3d {
    pub corners: [Vec3; 4],
    #[serde(default)]
    pub hidden_edges: u8,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RayLine {
    pub base: Vec3,
    pub dir: Vec3,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Viewport {
    pub center: Vec3,
    pub width: f64,
    pub height: f64,
    pub view_center: Vec2,
    pub view_height: f64,
    #[serde(default)]
    pub id: u32,
    #[serde(default)]
    pub locked: bool,
    #[serde(default)]
    pub frozen_layers: Vec<String>,
    /// Per-viewport layer colour overrides (VP Color in the Layer Properties Manager).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_colors: Vec<(String, Color)>,
    /// The paper-space entity whose outline clips the viewport (a closed polyline, circle,
    /// ellipse or spline; DXF group 340). `None`: the rectangle `center`/`width`/`height`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub clip: Option<Handle>,
    /// Display on (MVIEW ON/OFF). An off viewport shows its border but no model space.
    #[serde(default = "yes")]
    pub on: bool,
}
impl Default for Viewport {
    fn default() -> Self {
        Viewport {
            center: Vec3::ZERO,
            width: 1.0,
            height: 1.0,
            view_center: Vec2::ZERO,
            view_height: 1.0,
            id: 0,
            locked: false,
            frozen_layers: Vec::new(),
            layer_colors: Vec::new(),
            clip: None,
            on: true,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Image {
    pub insert: Vec3,
    /// One pixel's width (U) and height (V) vectors in drawing units.
    pub u: Vec3,
    pub v: Vec3,
    /// Image size in pixels.
    pub size: Vec2,
    /// The image file (its IMAGEDEF object's path).
    pub path: String,
    /// The image definition's name (its key in the drawing's image dictionary).
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Clip boundary in pixel coordinates (origin at the top-left corner of the image, y down):
    /// two opposite corners of a rectangle, or a polygon's vertices. Empty: the whole image.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub clip: Vec<Vec2>,
    /// The clip boundary is applied.
    #[serde(default)]
    pub clipping: bool,
    /// Display flags: 1 show the image, 2 show it when not aligned with the screen, 4 use the
    /// clip boundary, 8 transparency on.
    #[serde(default = "image_display")]
    pub display: u16,
    /// Brightness, contrast and fade, 0..=100.
    #[serde(default = "image_fifty")]
    pub brightness: u8,
    #[serde(default = "image_fifty")]
    pub contrast: u8,
    #[serde(default)]
    pub fade: u8,
    /// The definition's default size of one pixel and its resolution unit (0 none, 2 cm, 5 inch).
    #[serde(default = "image_pixel")]
    pub pixel_size: Vec2,
    #[serde(default)]
    pub resolution_units: u8,
}
fn image_display() -> u16 {
    7
}
fn image_fifty() -> u8 {
    50
}
fn image_pixel() -> Vec2 {
    Vec2::new(1.0, 1.0)
}
impl Default for Image {
    fn default() -> Self {
        Image {
            insert: Vec3::ZERO,
            u: Vec3::new(1.0, 0.0, 0.0),
            v: Vec3::new(0.0, 1.0, 0.0),
            size: Vec2::new(1.0, 1.0),
            path: String::new(),
            name: String::new(),
            clip: Vec::new(),
            clipping: false,
            display: image_display(),
            brightness: image_fifty(),
            contrast: image_fifty(),
            fade: 0,
            pixel_size: image_pixel(),
            resolution_units: 0,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Wipeout {
    pub boundary: Vec<Vec2>,
}
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TableCell {
    pub text: String,
    #[serde(default)]
    pub merged: Option<(u32, u32)>,
    /// The cell's own cell style; empty: its row's.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub style: String,
    /// A formula (`=Sum(A1:B3)`); `text` holds its value.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
    /// The cell's own text height (DXF 140 in the cell record), before the table's scale;
    /// `None`: its cell style's.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text_height: Option<f64>,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Table {
    pub insert: Vec3,
    pub col_widths: Vec<f64>,
    pub row_heights: Vec<f64>,
    pub cells: Vec<Vec<TableCell>>,
    #[serde(default = "standard")]
    pub style: String,
    #[serde(default = "default_text_h")]
    pub text_height: f64,
    /// First row is a title (merged across, larger text).
    #[serde(default)]
    pub title: bool,
    /// The row after the title holds column headers.
    #[serde(default)]
    pub header: bool,
    /// The cell style of each row; missing or empty: by position (Title, Header, Data).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub row_styles: Vec<String>,
    /// Table breaking (TABLEBREAK): the table drawn in several parts.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub breaks: Option<crate::TableBreak>,
    /// The CSV file the cells come from (DATALINK).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data_link: Option<crate::DataLink>,
}
fn default_text_h() -> f64 {
    0.18
}
mod region;
pub use region::*;

/// A raw DXF group: kept for round-tripping entities we do not model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawTag {
    pub code: i32,
    pub value: String,
}
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Unknown {
    pub dxf_type: String,
    pub tags: Vec<RawTag>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum EntityKind {
    Line(Line),
    Point(Point),
    Circle(Circle),
    Arc(Arc),
    Ellipse(Ellipse),
    LwPolyline(LwPolyline),
    Polyline3d(Polyline3d),
    Spline(GSpline),
    Helix(Helix),
    Ray(RayLine),
    XLine(RayLine),
    Text(Text),
    MText(MText),
    AttDef(Attrib),
    Insert(Insert),
    Dimension(Dimension),
    MLine(MLine),
    Leader(Leader),
    MLeader(MLeader),
    Hatch(Hatch),
    Solid(Solid),
    Trace(Solid),
    Face3d(Face3d),
    Region(Region),
    Viewport(Viewport),
    Image(Image),
    Wipeout(Wipeout),
    Table(Table),
    Tolerance(crate::Tolerance),
    Unknown(Unknown),
}

/// A primitive the entity decomposes into, for snapping, selection, trimming and rendering.
#[derive(Clone, Debug, PartialEq)]
pub enum Prim {
    Seg(Segment),
    Circle(GCircle),
    Ellipse(GEllipse),
    Spline(GSpline),
    Point(Vec2),
    /// An infinite line (`ray == false`) or a ray from `base` along `dir`.
    Infinite {
        base: Vec2,
        dir: Vec2,
        ray: bool,
    },
    /// A filled polygon (solids, wide polylines, hatch fill) — the outline is also selectable.
    Fill(Vec<Vec2>),
}

impl EntityKind {
    /// AutoCAD's object type name as shown in the Properties palette.
    pub fn type_name(&self) -> &'static str {
        match self {
            EntityKind::Line(_) => "Line",
            EntityKind::Point(_) => "Point",
            EntityKind::Circle(_) => "Circle",
            EntityKind::Arc(_) => "Arc",
            EntityKind::Ellipse(_) => "Ellipse",
            EntityKind::LwPolyline(_) => "Polyline",
            EntityKind::Polyline3d(_) => "3D Polyline",
            EntityKind::Spline(_) => "Spline",
            EntityKind::Helix(_) => "Helix",
            EntityKind::Ray(_) => "Ray",
            EntityKind::XLine(_) => "Xline",
            EntityKind::Text(_) => "Text",
            EntityKind::MText(_) => "MText",
            EntityKind::AttDef(_) => "Attribute Definition",
            EntityKind::Insert(_) => "Block Reference",
            EntityKind::Dimension(_) => "Dimension",
            EntityKind::MLine(_) => "MLine",
            EntityKind::Leader(_) => "Leader",
            EntityKind::MLeader(_) => "Multileader",
            EntityKind::Hatch(_) => "Hatch",
            EntityKind::Solid(_) => "2D Solid",
            EntityKind::Trace(_) => "Trace",
            EntityKind::Face3d(_) => "3D Face",
            EntityKind::Region(_) => "Region",
            EntityKind::Viewport(_) => "Viewport",
            EntityKind::Image(_) => "Raster Image",
            EntityKind::Wipeout(_) => "Wipeout",
            EntityKind::Table(_) => "Table",
            EntityKind::Tolerance(_) => "Tolerance",
            EntityKind::Unknown(_) => "Proxy",
        }
    }
    /// DXF entity type name.
    pub fn dxf_name(&self) -> &str {
        match self {
            EntityKind::Line(_) => "LINE",
            EntityKind::Point(_) => "POINT",
            EntityKind::Circle(_) => "CIRCLE",
            EntityKind::Arc(_) => "ARC",
            EntityKind::Ellipse(_) => "ELLIPSE",
            EntityKind::LwPolyline(_) => "LWPOLYLINE",
            EntityKind::Polyline3d(_) => "POLYLINE",
            EntityKind::Spline(_) => "SPLINE",
            EntityKind::Helix(_) => "HELIX",
            EntityKind::Ray(_) => "RAY",
            EntityKind::XLine(_) => "XLINE",
            EntityKind::Text(_) => "TEXT",
            EntityKind::MText(_) => "MTEXT",
            EntityKind::AttDef(_) => "ATTDEF",
            EntityKind::Insert(_) => "INSERT",
            EntityKind::Dimension(_) => "DIMENSION",
            EntityKind::MLine(_) => "MLINE",
            EntityKind::Leader(_) => "LEADER",
            EntityKind::MLeader(_) => "MULTILEADER",
            EntityKind::Hatch(_) => "HATCH",
            EntityKind::Solid(_) => "SOLID",
            EntityKind::Trace(_) => "TRACE",
            EntityKind::Face3d(_) => "3DFACE",
            EntityKind::Region(_) => "REGION",
            EntityKind::Viewport(_) => "VIEWPORT",
            EntityKind::Image(_) => "IMAGE",
            EntityKind::Wipeout(_) => "WIPEOUT",
            EntityKind::Table(_) => "ACAD_TABLE",
            EntityKind::Tolerance(_) => "TOLERANCE",
            EntityKind::Unknown(u) => u.dxf_type.as_str(),
        }
    }

    /// Geometric primitives in WCS (2D projection). Text/blocks/dimensions return their
    /// insertion points only; their full geometry comes from the renderer's expansion.
    pub fn prims(&self) -> Vec<Prim> {
        match self {
            EntityKind::Line(l) => vec![Prim::Seg(Segment::Line(GLine::new(l.a.xy(), l.b.xy())))],
            EntityKind::Point(p) => vec![Prim::Point(p.p.xy())],
            EntityKind::Circle(c) => vec![Prim::Circle(GCircle::new(c.center.xy(), c.radius))],
            EntityKind::Arc(a) => vec![Prim::Seg(Segment::Arc { arc: GArc::new(a.center.xy(), a.radius, a.start, a.end), ccw: true })],
            EntityKind::Ellipse(e) => {
                vec![Prim::Ellipse(GEllipse { center: e.center.xy(), major: e.major.xy(), ratio: e.ratio, start: e.start, end: e.end })]
            }
            EntityKind::LwPolyline(p) => {
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                pl.segments().into_iter().map(Prim::Seg).collect()
            }
            EntityKind::Polyline3d(p) => {
                let mut v: Vec<Prim> =
                    p.points.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))).collect();
                if p.closed
                    && let (Some(a), Some(b)) = (p.points.last(), p.points.first())
                {
                    v.push(Prim::Seg(Segment::Line(GLine::new(a.xy(), b.xy()))));
                }
                v
            }
            EntityKind::Spline(s) => vec![Prim::Spline(s.clone())],
            EntityKind::Helix(h) => vec![Prim::Spline(h.spline())],
            EntityKind::Ray(r) => vec![Prim::Infinite { base: r.base.xy(), dir: r.dir.xy().normalized(), ray: true }],
            EntityKind::XLine(r) => vec![Prim::Infinite { base: r.base.xy(), dir: r.dir.xy().normalized(), ray: false }],
            EntityKind::Solid(s) | EntityKind::Trace(s) => {
                let c = &s.corners;
                vec![Prim::Fill(vec![c[0].xy(), c[1].xy(), c[3].xy(), c[2].xy()])]
            }
            EntityKind::Face3d(f) => {
                let c = &f.corners;
                let pts = [c[0].xy(), c[1].xy(), c[2].xy(), c[3].xy(), c[0].xy()];
                pts.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(*w.first()?, *w.get(1)?))))).collect()
            }
            EntityKind::Region(r) => r.segments().into_iter().map(Prim::Seg).collect(),
            EntityKind::Hatch(h) => {
                h.loops.iter().flat_map(|l| Polyline { vertices: l.vertices.clone(), closed: true }.segments()).map(Prim::Seg).collect()
            }
            EntityKind::Wipeout(w) => {
                let mut pts = w.boundary.clone();
                if let Some(f) = pts.first().copied() {
                    pts.push(f);
                }
                pts.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(*w.first()?, *w.get(1)?))))).collect()
            }
            EntityKind::Viewport(v) => {
                let c = v.center.xy();
                let h = Vec2::new(v.width / 2.0, v.height / 2.0);
                let b = Bounds2::new(c - h, c + h).corners();
                (0..4).map(|i| Prim::Seg(Segment::Line(GLine::new(b[i], b[(i + 1) % 4])))).collect()
            }
            EntityKind::Leader(l) => {
                l.vertices.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))).collect()
            }
            EntityKind::Text(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::MText(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::AttDef(a) => vec![Prim::Point(a.text.insert.xy())],
            EntityKind::Insert(i) => vec![Prim::Point(i.insert.xy())],
            EntityKind::Dimension(d) => vec![Prim::Point(d.defpt.xy())],
            EntityKind::MLine(m) => m.segments().into_iter().map(Prim::Seg).collect(),
            EntityKind::MLeader(m) => m
                .leaders
                .iter()
                .flat_map(|l| l.windows(2).filter_map(|w| Some(Prim::Seg(Segment::Line(GLine::new(w.first()?.xy(), w.get(1)?.xy()))))))
                .chain(m.block.iter().map(|b| Prim::Point(b.insert.xy())))
                .collect(),
            EntityKind::Image(i) => vec![Prim::Point(i.insert.xy())],
            EntityKind::Table(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::Tolerance(t) => vec![Prim::Point(t.insert.xy())],
            EntityKind::Unknown(_) => Vec::new(),
        }
    }

    /// Apply a 2D affine transform (move/rotate/uniform scale/mirror). Non-uniform scale turns
    /// circles into ellipses only where the kind supports it; otherwise radii use the mean scale.
    pub fn transform(&mut self, m: &Mat3) {
        let t3 = |p: &mut Vec3| {
            let q = m.apply(p.xy());
            p.x = q.x;
            p.y = q.y;
        };
        let s = m.scale_factor();
        let rot = m.rotation();
        let mirror = m.is_mirroring();
        match self {
            EntityKind::Line(l) => {
                t3(&mut l.a);
                t3(&mut l.b);
            }
            EntityKind::Point(p) => t3(&mut p.p),
            EntityKind::Circle(c) => {
                t3(&mut c.center);
                c.radius *= s;
            }
            EntityKind::Arc(a) => {
                let sp = m.apply(Vec2::polar(a.center.xy(), a.radius, a.start));
                let ep = m.apply(Vec2::polar(a.center.xy(), a.radius, a.end));
                t3(&mut a.center);
                a.radius *= s;
                let c = a.center.xy();
                if mirror {
                    a.start = c.angle_to(ep);
                    a.end = c.angle_to(sp);
                } else {
                    a.start = c.angle_to(sp);
                    a.end = c.angle_to(ep);
                }
            }
            EntityKind::Ellipse(e) => {
                let sp = m.apply(e.center.xy() + e.major.xy() * e.start.cos() + e.major.xy().perp() * (e.ratio * e.start.sin()));
                let ep = m.apply(e.center.xy() + e.major.xy() * e.end.cos() + e.major.xy().perp() * (e.ratio * e.end.sin()));
                t3(&mut e.center);
                let mj = m.apply_vec(e.major.xy());
                e.major = mj.to3(0.0);
                let full = (cadcraft_geom::ccw_sweep(e.start, e.end) - cadcraft_geom::TAU).abs() < 1e-9;
                if !full {
                    let ge = GEllipse { center: e.center.xy(), major: mj, ratio: e.ratio, start: 0.0, end: cadcraft_geom::TAU };
                    let (ps, pe) = (ge.param_of(sp), ge.param_of(ep));
                    if mirror {
                        e.start = pe;
                        e.end = ps;
                    } else {
                        e.start = ps;
                        e.end = pe;
                    }
                }
            }
            EntityKind::LwPolyline(p) => {
                for v in &mut p.vertices {
                    v.p = m.apply(v.p);
                    v.start_width *= s;
                    v.end_width *= s;
                    if mirror {
                        v.bulge = -v.bulge;
                    }
                }
                p.const_width *= s;
            }
            EntityKind::Polyline3d(p) => p.points.iter_mut().for_each(t3),
            EntityKind::Spline(sp) => sp.transform(m),
            EntityKind::Helix(h) => {
                let sp = m.apply(h.start_point().xy());
                t3(&mut h.center);
                h.base_radius *= s;
                h.top_radius *= s;
                h.height *= s;
                h.start_angle = h.center.xy().angle_to(sp);
                // Mirrored, the twist turns the other way.
                h.ccw ^= mirror;
            }
            EntityKind::Ray(r) | EntityKind::XLine(r) => {
                t3(&mut r.base);
                r.dir = m.apply_vec(r.dir.xy()).normalized().to3(0.0);
            }
            EntityKind::Text(t) => transform_text(t, m, s, rot, mirror),
            EntityKind::AttDef(a) => {
                transform_text(&mut a.text, m, s, rot, mirror);
                a.transform_multiline(m);
            }
            EntityKind::MText(t) => {
                t3(&mut t.insert);
                t.height *= s;
                t.width *= s;
                if let Some(c) = &mut t.columns {
                    c.scale(s);
                }
                t.rotation = cadcraft_geom::norm_angle(t.rotation + rot);
            }
            EntityKind::Insert(i) => {
                t3(&mut i.insert);
                i.scale.x *= s;
                i.scale.y *= s;
                i.scale.z *= s;
                i.rotation = cadcraft_geom::norm_angle(i.rotation + rot);
                if mirror {
                    i.scale.y = -i.scale.y;
                    i.rotation = cadcraft_geom::norm_angle(rot - (i.rotation - rot));
                }
                for a in &mut i.attribs {
                    transform_text(&mut a.text, m, s, rot, false);
                    a.transform_multiline(m);
                }
                i.col_spacing *= s;
                i.row_spacing *= s;
            }
            EntityKind::Dimension(d) => {
                for p in [&mut d.defpt, &mut d.text_mid, &mut d.p13, &mut d.p14, &mut d.p15, &mut d.p16] {
                    t3(p);
                }
                if let DimKind::Linear { rotation } = &mut d.kind {
                    *rotation = cadcraft_geom::norm_angle(*rotation + rot);
                }
                d.marks.transform(&t3, rot, s, mirror);
                d.block = None;
            }
            EntityKind::MLine(ml) => {
                ml.vertices.iter_mut().for_each(|v| t3(&mut v.p));
                ml.vertices.iter_mut().flat_map(|v| v.cuts.iter_mut()).for_each(|c| (c.start, c.end) = (c.start * s, c.end * s));
                // Mirrored, the elements change sides.
                ml.scale *= if mirror { -s } else { s };
            }
            EntityKind::Leader(l) => l.vertices.iter_mut().for_each(t3),
            EntityKind::MLeader(ml) => {
                for l in &mut ml.leaders {
                    l.iter_mut().for_each(t3);
                }
                t3(&mut ml.landing);
                if let Some(t) = &mut ml.text {
                    t3(&mut t.insert);
                    t.height *= s;
                    t.width *= s;
                    if let Some(c) = &mut t.columns {
                        c.scale(s);
                    }
                }
                if let Some(b) = &mut ml.block {
                    // Transformed exactly like a block reference.
                    let mut k = EntityKind::Insert(b.clone());
                    k.transform(m);
                    if let EntityKind::Insert(nb) = k {
                        *b = nb;
                    }
                }
            }
            EntityKind::Hatch(h) => {
                for l in &mut h.loops {
                    for v in &mut l.vertices {
                        v.p = m.apply(v.p);
                        if mirror {
                            v.bulge = -v.bulge;
                        }
                    }
                }
                h.scale *= s;
                h.angle = cadcraft_geom::norm_angle(h.angle + rot);
                h.origin = m.apply(h.origin);
            }
            EntityKind::Solid(so) | EntityKind::Trace(so) => so.corners.iter_mut().for_each(t3),
            EntityKind::Face3d(f) => f.corners.iter_mut().for_each(t3),
            EntityKind::Region(r) => {
                for v in r.loops.iter_mut().flat_map(|l| l.vertices.iter_mut()) {
                    v.p = m.apply(v.p);
                    if mirror {
                        v.bulge = -v.bulge;
                    }
                }
            }
            EntityKind::Viewport(v) => {
                t3(&mut v.center);
                v.width *= s;
                v.height *= s;
            }
            EntityKind::Image(i) => {
                t3(&mut i.insert);
                i.u = m.apply_vec(i.u.xy()).to3(0.0);
                i.v = m.apply_vec(i.v.xy()).to3(0.0);
            }
            EntityKind::Wipeout(w) => w.boundary.iter_mut().for_each(|p| *p = m.apply(*p)),
            EntityKind::Table(t) => {
                if let Some(b) = &mut t.breaks {
                    b.height *= s;
                    b.spacing *= s;
                    b.heights.iter_mut().for_each(|h| *h *= s);
                    b.offsets.iter_mut().for_each(|o| *o = m.apply_vec(*o));
                }
                t3(&mut t.insert);
                t.col_widths.iter_mut().for_each(|w| *w *= s);
                t.row_heights.iter_mut().for_each(|h| *h *= s);
                t.text_height *= s;
            }
            EntityKind::Tolerance(t) => {
                t3(&mut t.insert);
                // Like text with MIRRTEXT = 0: a mirrored frame keeps reading left to right.
                let a = if mirror { t.rotation() } else { t.rotation() + rot };
                t.dir = Vec2::from_angle(cadcraft_geom::norm_angle(a)).to3(0.0);
            }
            EntityKind::Unknown(_) => {}
        }
    }

    /// Grip points (editable defining points), in display order.
    pub fn grips(&self) -> Vec<Vec2> {
        // A broken table has a grip for each part.
        if let EntityKind::Table(t) = self {
            return t.grip_points();
        }
        match self {
            EntityKind::Line(l) => vec![l.a.xy(), l.a.xy().mid(l.b.xy()), l.b.xy()],
            EntityKind::Point(p) => vec![p.p.xy()],
            EntityKind::Circle(c) => {
                let o = c.center.xy();
                vec![o, o + Vec2::X * c.radius, o + Vec2::Y * c.radius, o - Vec2::X * c.radius, o - Vec2::Y * c.radius]
            }
            EntityKind::Arc(a) => {
                let g = GArc::new(a.center.xy(), a.radius, a.start, a.end);
                vec![g.start_point(), g.mid_point(), g.end_point(), a.center.xy()]
            }
            EntityKind::Ellipse(e) => {
                let c = e.center.xy();
                let mj = e.major.xy();
                let mn = mj.perp() * e.ratio;
                vec![c, c + mj, c + mn, c - mj, c - mn]
            }
            EntityKind::LwPolyline(p) => {
                let mut g: Vec<Vec2> = Vec::new();
                let pl = Polyline { vertices: p.vertices.clone(), closed: p.closed };
                // Compute the segments once, not once per vertex: rebuilding the whole segment
                // vector inside the loop is quadratic in the vertex count (CWE-407).
                let segs = pl.segments();
                for (i, v) in p.vertices.iter().enumerate() {
                    g.push(v.p);
                    if let Some(s) = segs.get(i) {
                        g.push(s.mid());
                    }
                }
                g
            }
            EntityKind::Polyline3d(p) => p.points.iter().map(|q| q.xy()).collect(),
            EntityKind::Spline(s) => {
                if s.fit.is_empty() {
                    s.control.clone()
                } else {
                    s.fit.clone()
                }
            }
            EntityKind::Helix(h) => vec![h.center.xy(), h.start_point().xy(), h.end_point().xy()],
            EntityKind::Ray(r) | EntityKind::XLine(r) => vec![r.base.xy(), r.base.xy() + r.dir.xy()],
            EntityKind::Text(t) => vec![t.justify_point()],
            EntityKind::MText(t) => vec![t.insert.xy()],
            EntityKind::AttDef(a) => vec![a.text.justify_point()],
            EntityKind::Insert(i) => std::iter::once(i.insert.xy()).chain(i.attribs.iter().map(|a| a.text.justify_point())).collect(),
            EntityKind::Dimension(d) => vec![d.p13.xy(), d.p14.xy(), d.defpt.xy(), d.text_mid.xy()],
            EntityKind::MLine(m) => m.points(),
            EntityKind::Leader(l) => l.vertices.iter().map(|v| v.xy()).collect(),
            EntityKind::MLeader(m) => m.leaders.iter().flat_map(|l| l.iter().map(|v| v.xy())).chain(std::iter::once(m.landing.xy())).collect(),
            EntityKind::Hatch(h) => {
                let b = Bounds2::from_points(h.loops.iter().flat_map(|l| l.vertices.iter().map(|v| v.p)));
                vec![b.center()]
            }
            EntityKind::Solid(s) | EntityKind::Trace(s) => s.corners.iter().map(|c| c.xy()).collect(),
            EntityKind::Face3d(f) => f.corners.iter().map(|c| c.xy()).collect(),
            EntityKind::Region(r) => vec![r.bounds().center()],
            EntityKind::Viewport(v) => {
                let c = v.center.xy();
                let h = Vec2::new(v.width / 2.0, v.height / 2.0);
                Bounds2::new(c - h, c + h).corners().to_vec()
            }
            EntityKind::Image(i) => vec![i.insert.xy()],
            EntityKind::Wipeout(w) => w.boundary.clone(),
            EntityKind::Table(t) => vec![t.insert.xy()],
            EntityKind::Tolerance(t) => vec![t.insert.xy()],
            EntityKind::Unknown(_) => Vec::new(),
        }
    }
}

fn transform_text(t: &mut Text, m: &Mat3, s: f64, rot: f64, mirror: bool) {
    let q = m.apply(t.insert.xy());
    t.insert.x = q.x;
    t.insert.y = q.y;
    if let Some(a) = &mut t.align_pt {
        let q = m.apply(a.xy());
        a.x = q.x;
        a.y = q.y;
    }
    t.height *= s;
    // MIRRTEXT = 0 (the default): mirrored text keeps reading left to right.
    t.rotation = cadcraft_geom::norm_angle(t.rotation + if mirror { 0.0 } else { rot });
}

#[cfg(test)]
mod grip_tests {
    use super::*;
    use cadcraft_geom::PolyVertex;

    #[test]
    fn handle_parse_hex_accepts_only_hex_digits() {
        assert_eq!(Handle::parse_hex("1a"), Some(Handle(0x1A)));
        assert_eq!(Handle::parse_hex(" 1A "), Some(Handle(0x1A)));
        for bad in ["+1A", "-1A", "", "+", "1 A"] {
            assert_eq!(Handle::parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn polyline_grips_are_vertices_and_midpoints() {
        // Grips are each vertex plus the midpoint of each segment; the segment vector is computed
        // once rather than rebuilt per vertex (CWE-407, row 35).
        let verts: Vec<PolyVertex> = [(0.0, 0.0), (10.0, 0.0), (10.0, 10.0)].iter().map(|&(x, y)| PolyVertex::new(Vec2::new(x, y))).collect();
        let e = EntityKind::LwPolyline(LwPolyline { vertices: verts, closed: false, const_width: 0.0, elevation: 0.0, plinegen: false });
        let g = e.grips();
        // 3 vertices + 2 segment midpoints.
        assert_eq!(g.len(), 5, "{g:?}");
        assert!(g.contains(&Vec2::new(0.0, 0.0)) && g.contains(&Vec2::new(5.0, 0.0)));
    }
}
