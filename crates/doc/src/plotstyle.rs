//! Plot style tables: how objects print.
//!
//! A colour-dependent table (`.ctb`) has one plot style per index colour 1..=255: an object
//! prints with the style of its colour. A named table (`.stb`) holds styles that layers and
//! objects name directly. Each style can replace the object's colour, fade it (screening), turn
//! it grey, and replace its linetype and lineweight.
//!
//! CADCraft's built-in tables are our own definitions, not copies of any vendor's files:
//! [`DEFAULT_CTB`] keeps the object's properties, [`MONOCHROME_CTB`] prints everything black
//! and [`GRAYSCALE_CTB`] prints every colour as the grey of its luminance; the named
//! [`DEFAULT_STB`] offers `Normal`, `Black`, `Screened 50%` and `Thick`, and
//! [`MONOCHROME_STB`] prints `Normal` black.
//!
//! Tables are saved as files in our own JSON format (see `docs/plot-styles.md`):
//! [`PlotStyleTable::to_file_text`] and [`PlotStyleTable::from_file_text`].

use cadcraft_color::Rgb;
use serde::{Deserialize, Serialize};

/// Built-in colour-dependent table that prints objects with their own properties.
pub const DEFAULT_CTB: &str = "default.ctb";
/// Built-in colour-dependent table that prints everything black.
pub const MONOCHROME_CTB: &str = "monochrome.ctb";
/// Built-in colour-dependent table that prints every colour as a grey of its luminance.
pub const GRAYSCALE_CTB: &str = "grayscale.ctb";

/// Built-in named table: `Normal` (object properties), `Black`, `Screened 50%` and `Thick`.
pub const DEFAULT_STB: &str = "default.stb";
/// Built-in named table whose `Normal` style prints black.
pub const MONOCHROME_STB: &str = "monochrome.stb";

/// The names of the built-in tables.
pub const BUILTIN_PLOT_STYLE_TABLES: &[&str] = &[DEFAULT_CTB, MONOCHROME_CTB, GRAYSCALE_CTB, DEFAULT_STB, MONOCHROME_STB];

/// The style every named table has, which keeps the object's properties unless redefined.
pub const NORMAL_STYLE: &str = "Normal";

/// Most styles a named table may hold.
pub const MAX_NAMED_STYLES: usize = 1024;
/// Largest table file we read, in bytes.
pub const MAX_TABLE_FILE: usize = 4 << 20;
/// The JSON key that marks a file as one of our plot style tables.
pub const TABLE_FILE_MARKER: &str = "cadcraftPlotStyleTable";
/// Format version of our table files.
const FILE_VERSION: u64 = 1;

/// Number of styles in a colour-dependent table (one per index colour 1..=255).
pub const CTB_STYLES: usize = 255;

/// Line end style of a plot style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum EndStyle {
    #[default]
    Object,
    Butt,
    Square,
    Round,
    Diamond,
}

/// Line join style of a plot style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum JoinStyle {
    #[default]
    Object,
    Miter,
    Bevel,
    Round,
    Diamond,
}

/// Fill style of a plot style (for filled areas).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum FillStyle {
    #[default]
    Object,
    Solid,
    Checkerboard,
    Crosshatch,
    Diamonds,
    HorizontalBars,
    SlantLeft,
    SlantRight,
    SquareDots,
    VerticalBars,
}

/// One plot style. Serialized fields left at "use the object's" are omitted.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlotStyle {
    pub name: String,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub description: String,
    /// Output colour (`"#rrggbb"` in JSON); `None` uses the object's colour.
    #[serde(with = "rgb_hex", skip_serializing_if = "Option::is_none")]
    pub color: Option<Rgb>,
    /// Print the colour as the grey of its luminance.
    #[serde(skip_serializing_if = "is_false")]
    pub grayscale: bool,
    /// Ink intensity in percent, 0 (white) to 100 (full colour).
    #[serde(skip_serializing_if = "is_full")]
    pub screening: u8,
    /// Linetype name; `None` uses the object's linetype.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub linetype: Option<String>,
    /// Lineweight in hundredths of a millimetre (0..=211); `None` uses the object's lineweight.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub lineweight: Option<u16>,
    #[serde(skip_serializing_if = "is_default")]
    pub end: EndStyle,
    #[serde(skip_serializing_if = "is_default")]
    pub join: JoinStyle,
    #[serde(skip_serializing_if = "is_default")]
    pub fill: FillStyle,
}

fn is_false(b: &bool) -> bool {
    !*b
}
fn is_full(s: &u8) -> bool {
    *s >= 100
}
fn is_default<T: Default + PartialEq>(v: &T) -> bool {
    *v == T::default()
}

/// `Option<Rgb>` as `"#rrggbb"` (also read from `[r, g, b]`).
mod rgb_hex {
    use cadcraft_color::Rgb;
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S: Serializer>(c: &Option<Rgb>, s: S) -> Result<S::Ok, S::Error> {
        match c {
            Some(c) => s.serialize_str(&c.hex()),
            None => s.serialize_none(),
        }
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum Any {
        Hex(String),
        Triple(u8, u8, u8),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Option<Rgb>, D::Error> {
        Ok(match Option::<Any>::deserialize(d)? {
            None => None,
            Some(Any::Triple(r, g, b)) => Some(Rgb(r, g, b)),
            Some(Any::Hex(h)) => {
                Some(Rgb::parse_hex(h.trim().trim_start_matches('#')).ok_or_else(|| serde::de::Error::custom("colour must be \"#rrggbb\""))?)
            }
        })
    }
}

impl Default for PlotStyle {
    fn default() -> Self {
        PlotStyle::object("")
    }
}

impl PlotStyle {
    /// A style that keeps every object property.
    pub fn object(name: &str) -> PlotStyle {
        PlotStyle {
            name: name.into(),
            description: String::new(),
            color: None,
            grayscale: false,
            screening: 100,
            linetype: None,
            lineweight: None,
            end: EndStyle::Object,
            join: JoinStyle::Object,
            fill: FillStyle::Object,
        }
    }
    /// Whether the style leaves the colour alone.
    pub fn keeps_color(&self) -> bool {
        self.color.is_none() && !self.grayscale && self.screening >= 100
    }
    /// The printed colour of an object drawn in `rgb`: the style's colour (else `rgb`), turned
    /// grey when asked, then screened toward white.
    pub fn apply_color(&self, rgb: Rgb) -> Rgb {
        let c = self.color.unwrap_or(rgb);
        let c = if self.grayscale {
            let g = (c.luma().clamp(0.0, 1.0) * 255.0).round() as u8;
            Rgb(g, g, g)
        } else {
            c
        };
        let s = u16::from(self.screening.min(100));
        // Screening mixes the ink with the white paper: 100 % = full ink, 0 % = white.
        let screen = |v: u8| (255 - (255 - u16::from(v)) * s / 100) as u8;
        Rgb(screen(c.0), screen(c.1), screen(c.2))
    }
}

/// Whether a table maps colours or names to styles.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PlotStyleKind {
    /// Colour-dependent (`.ctb`): style `i - 1` applies to index colour `i`.
    #[default]
    ColorDependent,
    /// Named (`.stb`): styles are picked by name.
    Named,
}

/// A plot style table.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlotStyleTable {
    /// File name, such as `monochrome.ctb`.
    pub name: String,
    pub description: String,
    pub kind: PlotStyleKind,
    pub styles: Vec<PlotStyle>,
}

impl PlotStyleTable {
    /// A colour-dependent table whose style for index colour `i` is `f(i)` (else object
    /// properties), named `Color_i`.
    pub fn color_dependent(name: &str, description: &str, f: impl Fn(u8) -> Option<PlotStyle>) -> PlotStyleTable {
        let styles = (1..=255u8)
            .map(|i| {
                let label = format!("Color_{i}");
                let mut s = f(i).unwrap_or_else(|| PlotStyle::object(&label));
                s.name = label;
                s
            })
            .collect();
        PlotStyleTable { name: name.into(), description: description.into(), kind: PlotStyleKind::ColorDependent, styles }
    }

    /// A named table holding `Normal` (object properties) followed by `styles`.
    pub fn named(name: &str, description: &str, styles: Vec<PlotStyle>) -> PlotStyleTable {
        let mut all = vec![PlotStyle::object(NORMAL_STYLE)];
        all.extend(styles);
        PlotStyleTable { name: name.into(), description: description.into(), kind: PlotStyleKind::Named, styles: all }.sanitized()
    }

    /// A built-in table by name (case-insensitive; a `.ctb` extension may be left out).
    pub fn builtin(name: &str) -> Option<PlotStyleTable> {
        let n = name.trim().to_ascii_lowercase();
        let n = if n.ends_with(".ctb") || n.ends_with(".stb") { n } else { format!("{n}.ctb") };
        let black = Some(Rgb(0, 0, 0));
        match n.as_str() {
            DEFAULT_STB => Some(PlotStyleTable::named(
                DEFAULT_STB,
                "Normal keeps object properties; Black, Screened 50% and Thick",
                vec![
                    PlotStyle { name: "Black".into(), color: black, ..PlotStyle::default() },
                    PlotStyle { name: "Screened 50%".into(), screening: 50, ..PlotStyle::default() },
                    PlotStyle { name: "Thick".into(), lineweight: Some(50), ..PlotStyle::default() },
                ],
            )),
            MONOCHROME_STB => {
                let mut t = PlotStyleTable::named(
                    MONOCHROME_STB,
                    "Normal prints black; Screened 50% prints grey",
                    vec![PlotStyle { name: "Screened 50%".into(), color: black, screening: 50, ..PlotStyle::default() }],
                );
                if let Some(n) = t.styles.first_mut() {
                    n.color = black;
                }
                Some(t)
            }
            DEFAULT_CTB => Some(PlotStyleTable::color_dependent(DEFAULT_CTB, "Object colours, linetypes and lineweights", |_| None)),
            MONOCHROME_CTB => Some(PlotStyleTable::color_dependent(MONOCHROME_CTB, "Every colour prints black", |_| {
                Some(PlotStyle { color: Some(Rgb(0, 0, 0)), ..PlotStyle::default() })
            })),
            GRAYSCALE_CTB => Some(PlotStyleTable::color_dependent(GRAYSCALE_CTB, "Every colour prints as the grey of its luminance", |_| {
                Some(PlotStyle { grayscale: true, ..PlotStyle::default() })
            })),
            _ => None,
        }
    }

    /// Whether this table is colour-dependent.
    pub fn is_color_dependent(&self) -> bool {
        self.kind == PlotStyleKind::ColorDependent
    }

    /// The style for index colour `aci` (1..=255) of a colour-dependent table.
    pub fn by_color(&self, aci: u8) -> Option<&PlotStyle> {
        if !self.is_color_dependent() || aci == 0 {
            return None;
        }
        self.styles.get(usize::from(aci) - 1)
    }

    /// The style called `name` (case-insensitive).
    pub fn by_name(&self, name: &str) -> Option<&PlotStyle> {
        self.styles.iter().find(|s| s.name.eq_ignore_ascii_case(name))
    }

    /// The table with hostile or inconsistent values fixed: screening at most 100 %,
    /// lineweights at most 2.11 mm, names at most 255 characters; a colour-dependent table has
    /// exactly 255 styles named `Color_1`..`Color_255`; a named table has unique, non-empty
    /// style names (at most [`MAX_NAMED_STYLES`]) starting with `Normal`.
    pub fn sanitized(mut self) -> PlotStyleTable {
        let cut = |s: &str| s.chars().take(255).collect::<String>();
        self.name = cut(self.name.trim());
        self.description = cut(&self.description);
        for s in &mut self.styles {
            s.name = cut(s.name.trim());
            s.description = cut(&s.description);
            s.screening = s.screening.min(100);
            s.lineweight = s.lineweight.map(|w| w.min(211));
            s.linetype = s.linetype.take().map(|l| cut(l.trim())).filter(|l| !l.is_empty());
        }
        match self.kind {
            PlotStyleKind::ColorDependent => {
                self.styles.truncate(CTB_STYLES);
                while self.styles.len() < CTB_STYLES {
                    self.styles.push(PlotStyle::object(""));
                }
                for (i, s) in self.styles.iter_mut().enumerate() {
                    s.name = format!("Color_{}", i + 1);
                }
            }
            PlotStyleKind::Named => {
                let mut seen = std::collections::HashSet::new();
                self.styles.retain(|s| !s.name.is_empty() && seen.insert(s.name.to_lowercase()));
                self.styles.truncate(MAX_NAMED_STYLES);
                match self.styles.iter().position(|s| s.name.eq_ignore_ascii_case(NORMAL_STYLE)) {
                    Some(0) => {}
                    Some(i) => {
                        let n = self.styles.remove(i);
                        self.styles.insert(0, n);
                    }
                    None => {
                        self.styles.insert(0, PlotStyle::object(NORMAL_STYLE));
                        self.styles.truncate(MAX_NAMED_STYLES);
                    }
                }
                if let Some(n) = self.styles.first_mut() {
                    n.name = NORMAL_STYLE.into();
                }
            }
        }
        self
    }

    /// The table as a file in our own format: pretty-printed JSON with a format marker.
    pub fn to_file_text(&self) -> String {
        let mut v = serde_json::to_value(self).unwrap_or(serde_json::Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert(TABLE_FILE_MARKER.into(), FILE_VERSION.into());
        }
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    /// Read a table file written by [`PlotStyleTable::to_file_text`] (sanitized).
    pub fn from_file_text(text: &str) -> std::result::Result<PlotStyleTable, String> {
        if text.len() > MAX_TABLE_FILE {
            return Err("the plot style table file is too large".into());
        }
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not a CADCraft plot style table: {e}"))?;
        match v.get(TABLE_FILE_MARKER).and_then(serde_json::Value::as_u64) {
            Some(FILE_VERSION) => {}
            Some(n) => return Err(format!("plot style table format version {n} is newer than this CADCraft reads")),
            None => return Err(format!("not a CADCraft plot style table (no `{TABLE_FILE_MARKER}`)")),
        }
        let t: PlotStyleTable = serde_json::from_value(v).map_err(|e| format!("bad plot style table: {e}"))?;
        Ok(t.sanitized())
    }
}

/// Whether a page setup's table name means "no plot style table".
pub fn no_plot_style_table(name: &str) -> bool {
    let n = name.trim();
    n.is_empty() || n.eq_ignore_ascii_case("none")
}

/// The plot style table a page setup names, if it is one we know: the drawing's own tables
/// first, then the built-in ones. `None` for no table (empty or "None") and for tables we don't
/// have.
pub fn plot_style_table(d: &crate::Drawing, name: &str) -> Option<PlotStyleTable> {
    if no_plot_style_table(name) {
        return None;
    }
    let n = name.trim();
    d.plot_style_tables.iter().find(|t| t.name.eq_ignore_ascii_case(n)).cloned().or_else(|| PlotStyleTable::builtin(n))
}

/// The names of every table a drawing can use: its own, then the built-in ones it doesn't
/// shadow.
pub fn plot_style_table_names(d: &crate::Drawing) -> Vec<String> {
    let mut v: Vec<String> = d.plot_style_tables.iter().map(|t| t.name.clone()).collect();
    for b in BUILTIN_PLOT_STYLE_TABLES {
        if !v.iter().any(|n| n.eq_ignore_ascii_case(b)) {
            v.push((*b).to_string());
        }
    }
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_are_complete_color_dependent_tables() {
        for n in [DEFAULT_CTB, MONOCHROME_CTB, GRAYSCALE_CTB] {
            let t = PlotStyleTable::builtin(n).unwrap();
            assert_eq!(t.name, n);
            assert!(t.is_color_dependent());
            assert_eq!(t.styles.len(), CTB_STYLES);
            assert_eq!(t.by_color(1).unwrap().name, "Color_1");
            assert_eq!(t.by_color(255).unwrap().name, "Color_255");
            assert!(t.by_color(0).is_none());
        }
        assert!(PlotStyleTable::builtin("Monochrome").is_some());
        assert!(PlotStyleTable::builtin("nope.ctb").is_none());
        assert!(no_plot_style_table("") && no_plot_style_table(" None ") && !no_plot_style_table("monochrome.ctb"));
    }

    #[test]
    fn colour_transforms() {
        let mono = PlotStyleTable::builtin(MONOCHROME_CTB).unwrap();
        assert_eq!(mono.by_color(1).unwrap().apply_color(Rgb(255, 0, 0)), Rgb(0, 0, 0));
        let gray = PlotStyleTable::builtin(GRAYSCALE_CTB).unwrap();
        let g = gray.by_color(3).unwrap().apply_color(Rgb(0, 255, 0));
        assert!(g.0 == g.1 && g.1 == g.2 && g.0 > 128, "{g:?}");
        let half = PlotStyle { color: Some(Rgb(0, 0, 0)), screening: 50, ..PlotStyle::default() };
        assert_eq!(half.apply_color(Rgb(9, 9, 9)), Rgb(128, 128, 128));
        let none = PlotStyle { screening: 0, ..PlotStyle::default() };
        assert_eq!(none.apply_color(Rgb(10, 20, 30)), Rgb(255, 255, 255));
        assert!(PlotStyle::default().keeps_color() && !half.keeps_color());
    }

    #[test]
    fn named_builtins_and_sanitizing() {
        let t = PlotStyleTable::builtin("default.stb").unwrap();
        assert!(!t.is_color_dependent());
        assert_eq!(t.styles.first().unwrap().name, NORMAL_STYLE);
        assert!(t.by_name("black").is_some() && t.by_color(1).is_none());
        assert_eq!(PlotStyleTable::builtin(MONOCHROME_STB).unwrap().by_name("Normal").unwrap().color, Some(Rgb(0, 0, 0)));
        // Hostile values are clamped; names fixed.
        let bad = PlotStyleTable {
            name: " x.stb ".into(),
            kind: PlotStyleKind::Named,
            styles: vec![
                PlotStyle { name: "A".into(), screening: 250, lineweight: Some(9999), ..PlotStyle::default() },
                PlotStyle { name: "a".into(), ..PlotStyle::default() },
                PlotStyle { name: " ".into(), ..PlotStyle::default() },
            ],
            ..PlotStyleTable::default()
        }
        .sanitized();
        assert_eq!(bad.name, "x.stb");
        assert_eq!(bad.styles.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), ["Normal", "A"]);
        assert_eq!((bad.styles[1].screening, bad.styles[1].lineweight), (100, Some(211)));
        let short = PlotStyleTable { styles: vec![PlotStyle::object("zz")], ..PlotStyleTable::default() }.sanitized();
        assert_eq!(short.styles.len(), CTB_STYLES);
        assert_eq!(short.styles[0].name, "Color_1");
    }

    #[test]
    fn file_format_round_trip() {
        for name in BUILTIN_PLOT_STYLE_TABLES {
            let t = PlotStyleTable::builtin(name).unwrap();
            let text = t.to_file_text();
            assert!(text.contains("\"cadcraftPlotStyleTable\": 1"));
            assert_eq!(PlotStyleTable::from_file_text(&text).unwrap(), t, "{name}");
        }
        let mono = PlotStyleTable::builtin(MONOCHROME_CTB).unwrap().to_file_text();
        assert!(mono.contains("\"color\": \"#000000\""), "colours as hex");
        assert!(!mono.contains("screening"), "defaults are left out");
        assert!(PlotStyleTable::from_file_text("{}").is_err());
        assert!(PlotStyleTable::from_file_text("{\"cadcraftPlotStyleTable\": 9}").is_err());
        assert!(PlotStyleTable::from_file_text("not json").is_err());
        let rgb: PlotStyle = serde_json::from_value(serde_json::json!({"name": "x", "color": [1, 2, 3]})).unwrap();
        assert_eq!(rgb.color, Some(Rgb(1, 2, 3)));
        assert!(serde_json::from_value::<PlotStyle>(serde_json::json!({"color": "#zz0000"})).is_err());
    }

    #[test]
    fn drawing_tables_shadow_builtins() {
        let mut d = crate::Drawing::new_metric();
        assert!(plot_style_table(&d, "monochrome.ctb").unwrap().by_color(1).unwrap().color.is_some());
        d.plot_style_tables.push(PlotStyleTable::color_dependent("Monochrome.ctb", "", |_| None));
        assert!(plot_style_table(&d, "monochrome.ctb").unwrap().by_color(1).unwrap().color.is_none());
        let names = plot_style_table_names(&d);
        assert_eq!(names.iter().filter(|n| n.eq_ignore_ascii_case("monochrome.ctb")).count(), 1);
        assert!(names.contains(&"default.stb".to_string()));
    }

    #[test]
    fn json_round_trip_with_defaults() {
        let t = PlotStyleTable::builtin(GRAYSCALE_CTB).unwrap();
        let back: PlotStyleTable = serde_json::from_value(serde_json::to_value(&t).unwrap()).unwrap();
        assert_eq!(back, t);
        let s: PlotStyle = serde_json::from_value(serde_json::json!({"name": "Thick", "lineweight": 50})).unwrap();
        assert_eq!((s.screening, s.lineweight, s.color), (100, Some(50), None));
    }
}
