use super::{Color, Deserialize, Serialize, Vec2, Vec3, one, standard};

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
    /// Mirrored left to right (AutoCAD's Backwards; DXF 71 bit 2). New text takes it from its
    /// style; later style changes don't change it.
    #[serde(default)]
    pub backwards: bool,
    /// Mirrored top to bottom (AutoCAD's Upside down; DXF 71 bit 4).
    #[serde(default)]
    pub upside_down: bool,
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
    /// Drawing direction (DXF 72). CADCraft draws every MTEXT left to right; the direction is
    /// kept for the file.
    #[serde(default)]
    pub direction: MTextDirection,
    /// Background mask: a filled box behind the text (DXF 90, 63/421, 45); `None`: none.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub background: Option<MTextMask>,
}

fn attach_tl() -> u8 {
    1
}

/// MTEXT drawing direction (DXF 72: 1, 3, 5).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum MTextDirection {
    #[default]
    LeftToRight,
    TopToBottom,
    ByStyle,
}

impl MTextDirection {
    /// The DXF 72 code.
    pub fn code(self) -> i64 {
        match self {
            MTextDirection::LeftToRight => 1,
            MTextDirection::TopToBottom => 3,
            MTextDirection::ByStyle => 5,
        }
    }
    /// The direction of DXF 72 code `c` (anything else reads as left to right).
    pub fn from_code(c: i64) -> Self {
        match c {
            3 => MTextDirection::TopToBottom,
            5 => MTextDirection::ByStyle,
            _ => MTextDirection::LeftToRight,
        }
    }
}

/// An MTEXT background mask (AutoCAD's Background Mask): a box behind the text, filled with the
/// drawing's background or a color, hiding what was drawn before it.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MTextMask {
    /// Filled with the drawing's background color (DXF 90 = 2) rather than `color` (90 = 1).
    #[serde(default)]
    pub use_background: bool,
    /// The fill when not the background (DXF 63, 421).
    #[serde(default)]
    pub color: Color,
    /// The border offset factor (DXF 45, "fill box scale"), in text heights: 1 fits the text,
    /// 1.5 (AutoCAD's default) reaches half a text height further, split between the sides.
    #[serde(default = "mask_scale")]
    pub scale: f64,
}

/// AutoCAD's default border offset factor.
pub const MASK_SCALE: f64 = 1.5;

fn mask_scale() -> f64 {
    MASK_SCALE
}

impl Default for MTextMask {
    fn default() -> Self {
        MTextMask { use_background: true, color: Color::ByLayer, scale: MASK_SCALE }
    }
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
