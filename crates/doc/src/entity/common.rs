use super::{Color, Deserialize, Serialize, Vec3};

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

/// A raw DXF group: kept for round-tripping entities we do not model.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RawTag {
    pub code: i32,
    pub value: String,
}

impl Common {
    /// Whether "Match orientation to layout" is on for this annotative object (AutoCAD's
    /// `AcadAnnoPO` xdata = 1). AutoCAD draws such text upright and ignores its stored rotation.
    /// The object's own `AcadAnnotative` xdata never reaches [`Common::xdata`] (the reader drops
    /// it), so the `AcadAnnoPO` flag alone decides.
    pub fn matches_layout_orientation(&self) -> bool {
        let app = self.xdata.iter().position(|t| t.code == 1001 && t.value.trim().eq_ignore_ascii_case("AcadAnnoPO"));
        let Some(at) = app else { return false };
        self.xdata
            .iter()
            .skip(at + 1)
            .take_while(|t| t.code != 1001)
            .find(|t| t.code == 1070)
            .is_some_and(|t| t.value.trim().parse::<i64>().is_ok_and(|v| v != 0))
    }
}
