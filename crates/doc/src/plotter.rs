//! Plotter configurations: output devices with their paper sizes and printable areas.
//!
//! A configuration is a file in our own format (`*.plotter.json`, UTF-8 JSON with a format
//! marker, see `docs/plotters.md`). It names the output (PDF, PNG or SVG), the paper sizes the
//! device offers (each with the margins that bound its printable area), the raster resolution
//! and the default orientation. Binary `.pc3` files are not read.

use serde::{Deserialize, Serialize};

/// File-name suffix of plotter configuration files.
pub const PLOTTER_FILE_EXT: &str = ".plotter.json";
/// Largest plotter configuration file read (bytes).
pub const MAX_PLOTTER_FILE: usize = 1 << 20;
/// Most paper sizes one device keeps.
pub const MAX_DEVICE_PAPERS: usize = 512;
/// Longest device or paper name (characters).
pub const MAX_PLOTTER_NAME: usize = 100;
/// Largest paper side (mm), as for page setups.
pub const MAX_PAPER_MM: f64 = 5080.0;

const FILE_MARKER: &str = "cadcraftPlotter";
const FILE_VERSION: u64 = 1;

/// What a device writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlotOutput {
    #[default]
    Pdf,
    Png,
    Svg,
}

impl PlotOutput {
    pub fn as_str(self) -> &'static str {
        match self {
            PlotOutput::Pdf => "pdf",
            PlotOutput::Png => "png",
            PlotOutput::Svg => "svg",
        }
    }
    /// Parse `pdf`, `png` or `svg` (any case).
    pub fn parse(s: &str) -> Option<PlotOutput> {
        match s.trim().to_ascii_lowercase().as_str() {
            "pdf" => Some(PlotOutput::Pdf),
            "png" => Some(PlotOutput::Png),
            "svg" => Some(PlotOutput::Svg),
            _ => None,
        }
    }
}

/// Default sheet orientation of a device.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Orientation {
    Portrait,
    #[default]
    Landscape,
}

/// A paper size a device offers: size in millimetres (portrait: width <= height after
/// sanitizing) and margins (left, bottom, right, top; mm) bounding the printable area.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DevicePaper {
    pub name: String,
    pub width: f64,
    pub height: f64,
    pub margins: [f64; 4],
}

impl Default for DevicePaper {
    fn default() -> Self {
        DevicePaper { name: String::new(), width: 210.0, height: 297.0, margins: [0.0; 4] }
    }
}

impl DevicePaper {
    /// The paper with hostile values fixed: finite sizes within 1..=[`MAX_PAPER_MM`] mm
    /// (portrait), margins 0..=1000 mm that leave a printable area (else none), a name of at
    /// most [`MAX_PLOTTER_NAME`] characters. A sheet wider than tall is stored turned 90° and
    /// its margins turn with it, so the printable area the caller measured against the sheet it
    /// named is the one the stored portrait paper holds.
    pub fn sanitized(mut self) -> DevicePaper {
        let side = |v: f64| if v.is_finite() { v.clamp(1.0, MAX_PAPER_MM) } else { 1.0 };
        let (w, h) = (side(self.width), side(self.height));
        for m in &mut self.margins {
            *m = if m.is_finite() { m.clamp(0.0, 1000.0) } else { 0.0 };
        }
        if w > h {
            let [l, b, r, t] = self.margins;
            self.margins = [b, r, t, l];
        }
        self.width = w.min(h);
        self.height = w.max(h);
        let [l, b, r, t] = self.margins;
        if l + r >= self.width || b + t >= self.height {
            self.margins = [0.0; 4];
        }
        self.name = clean_name(&self.name);
        if self.name.is_empty() {
            self.name = format!("User ({:.2} x {:.2} MM)", self.width, self.height);
        }
        self
    }
}

/// A plotter configuration.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PlotterConfig {
    pub name: String,
    pub description: String,
    pub output: PlotOutput,
    /// Raster resolution (dots per inch) for PNG output.
    pub resolution: u32,
    pub orientation: Orientation,
    /// Name of the paper chosen when a page setup switches to this device.
    pub default_paper: String,
    pub papers: Vec<DevicePaper>,
}

impl Default for PlotterConfig {
    fn default() -> Self {
        PlotterConfig {
            name: String::new(),
            description: String::new(),
            output: PlotOutput::Pdf,
            resolution: 300,
            orientation: Orientation::Landscape,
            default_paper: String::new(),
            papers: Vec::new(),
        }
    }
}

/// A name trimmed, without control characters, at most [`MAX_PLOTTER_NAME`] characters.
fn clean_name(s: &str) -> String {
    s.trim().chars().filter(|c| !c.is_control()).take(MAX_PLOTTER_NAME).collect::<String>().trim().to_string()
}

/// Names compare ignoring case, spaces, `_` and `-`.
fn key(s: &str) -> String {
    s.chars().filter(|c| !c.is_whitespace() && *c != '_' && *c != '-').collect::<String>().to_ascii_lowercase()
}

impl PlotterConfig {
    /// The configuration with hostile values fixed: a clean name and description, resolution
    /// 50..=2400 dpi, at most [`MAX_DEVICE_PAPERS`] sanitized papers with unique names, and a
    /// default paper the device has (the first one when it names none of them).
    pub fn sanitized(mut self) -> PlotterConfig {
        self.name = clean_name(&self.name);
        self.description = self.description.chars().filter(|c| !c.is_control() || *c == '\n').take(1000).collect();
        self.resolution = self.resolution.clamp(50, 2400);
        let mut papers: Vec<DevicePaper> = Vec::new();
        for p in std::mem::take(&mut self.papers).into_iter().take(MAX_DEVICE_PAPERS) {
            let p = p.sanitized();
            if !papers.iter().any(|q| key(&q.name) == key(&p.name)) {
                papers.push(p);
            }
        }
        self.papers = papers;
        self.default_paper = match self.paper(&self.default_paper) {
            Some(p) => p.name.clone(),
            None => self.papers.first().map(|p| p.name.clone()).unwrap_or_default(),
        };
        self
    }

    /// The paper called `name` (ignoring case, spaces, `_` and `-`).
    pub fn paper(&self, name: &str) -> Option<&DevicePaper> {
        let k = key(name);
        if k.is_empty() {
            return None;
        }
        self.papers.iter().find(|p| key(&p.name) == k)
    }

    /// The paper a page setup gets when it switches to this device.
    pub fn default_paper(&self) -> Option<&DevicePaper> {
        self.paper(&self.default_paper).or_else(|| self.papers.first())
    }

    /// The configuration (sanitized) as a file in our own format: pretty-printed JSON with a
    /// format marker.
    pub fn to_file_text(&self) -> String {
        let mut v = serde_json::to_value(self.clone().sanitized()).unwrap_or(serde_json::Value::Null);
        if let Some(o) = v.as_object_mut() {
            o.insert(FILE_MARKER.into(), FILE_VERSION.into());
        }
        serde_json::to_string_pretty(&v).unwrap_or_default()
    }

    /// Read a file written by [`PlotterConfig::to_file_text`] (sanitized; a name is required).
    pub fn from_file_text(text: &str) -> std::result::Result<PlotterConfig, String> {
        if text.len() > MAX_PLOTTER_FILE {
            return Err("the plotter configuration file is too large".into());
        }
        let v: serde_json::Value = serde_json::from_str(text).map_err(|e| format!("not a CADCraft plotter configuration: {e}"))?;
        match v.get(FILE_MARKER).and_then(serde_json::Value::as_u64) {
            Some(FILE_VERSION) => {}
            Some(n) => return Err(format!("plotter configuration format version {n} is newer than this CADCraft reads")),
            None => return Err(format!("not a CADCraft plotter configuration (no `{FILE_MARKER}`)")),
        }
        let c: PlotterConfig = serde_json::from_value(v).map_err(|e| format!("bad plotter configuration: {e}"))?;
        let c = c.sanitized();
        if c.name.is_empty() {
            return Err("the plotter configuration has no name".into());
        }
        Ok(c)
    }
}

/// The file name a device is kept under: its name with characters that can't appear in file
/// names replaced by `_`, plus [`PLOTTER_FILE_EXT`]. `None` for an empty name.
pub fn plotter_file_name(name: &str) -> Option<String> {
    let base: String =
        clean_name(name).chars().map(|c| if c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '(' | ')' | '+' | ',') { c } else { '_' }).collect();
    let base = base.trim().trim_start_matches('.');
    (!base.is_empty()).then(|| format!("{base}{PLOTTER_FILE_EXT}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_and_hostile_values() {
        let c = PlotterConfig {
            name: "Office A3".into(),
            output: PlotOutput::Png,
            resolution: 1_000_000,
            default_paper: "nope".into(),
            papers: vec![
                DevicePaper { name: "A3 borderless".into(), width: 420.0, height: 297.0, margins: [0.0; 4] },
                DevicePaper { name: "a3-BORDERLESS".into(), ..DevicePaper::default() },
                DevicePaper { name: String::new(), width: f64::NAN, height: 1e12, margins: [f64::INFINITY, 9e9, -1.0, 5.0] },
            ],
            ..PlotterConfig::default()
        };
        let back = PlotterConfig::from_file_text(&c.to_file_text()).unwrap();
        assert_eq!(back.resolution, 2400);
        assert_eq!(back.papers.len(), 2, "duplicate names dropped");
        assert_eq!((back.papers[0].width, back.papers[0].height), (297.0, 420.0), "portrait");
        assert_eq!(back.default_paper, "A3 borderless");
        assert_eq!(back.papers[1].width, 1.0);
        assert_eq!(back.papers[1].height, MAX_PAPER_MM);
        assert_eq!(back.papers[1].margins, [0.0, 1000.0, 0.0, 5.0], "hostile margins clamped");
        let wide = DevicePaper { margins: [150.0, 0.0, 100.0, 0.0], ..DevicePaper::default() }.sanitized();
        assert_eq!(wide.margins, [0.0; 4], "margins wider than the paper are dropped");
        let turned = DevicePaper { name: "Banner".into(), width: 400.0, height: 200.0, margins: [150.0, 0.0, 150.0, 0.0] }.sanitized();
        assert_eq!((turned.width, turned.height), (200.0, 400.0), "portrait");
        assert_eq!(turned.margins, [0.0, 150.0, 0.0, 150.0], "margins turn with the sheet");
        assert!(back.paper("a3 BORDERLESS").is_some());
        assert!(PlotterConfig::from_file_text("{}").is_err());
        assert!(PlotterConfig::from_file_text(r#"{"cadcraftPlotter": 1}"#).is_err(), "needs a name");
        assert!(PlotterConfig::from_file_text(r#"{"cadcraftPlotter": 9, "name": "x"}"#).is_err());
        assert_eq!(plotter_file_name("../a/b:c").as_deref(), Some("___a_b_c.plotter.json"));
        assert_eq!(plotter_file_name("  "), None);
    }
}
